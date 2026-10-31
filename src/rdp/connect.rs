//! The connection sequence: TCP, TLS, the certificate check, NLA, and the
//! redirections a server can ask for on the way.

use std::time::Duration;

use ironrdp_connector::credssp::CredsspSequence;
use ironrdp_connector::sspi::generator::NetworkRequest;
use ironrdp_connector::{
    BitmapConfig, ClientConnector, ClientConnectorState, Config, ConnectionOutcome,
    ConnectionResult, ConnectorResult, Credentials, DesktopSize, ServerName,
};
use ironrdp_pdu::gcc;
use ironrdp_pdu::nego::SecurityProtocol;
use ironrdp_pdu::rdp::capability_sets::{
    MajorPlatformType, RailSupportLevel, client_codecs_capabilities,
};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_pdu::rdp::server_redirection::{ServerRedirectionFlags, ServerRedirectionPdu};
use ironrdp_tokio::{NetworkClient, TokioFramed};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use x509_cert::der::Encode as _;

use super::{Event, Failure, MAX_HOPS, Target, Trust, Unreachable, describe, rdstls};

pub(super) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Stream for T {}

/// The connection once TLS is up, ready for the session.
pub(super) type Framed = TokioFramed<Box<dyn Stream>>;

const TCP_TIMEOUT: Duration = Duration::from_secs(15);

/// Connect, following the server's redirections, until a session starts.
/// `redirect` carries a redirection the session itself asked for.
pub(super) async fn connect(
    target: &mut Target,
    mut redirect: Option<Box<ServerRedirectionPdu>>,
    events: &mpsc::Sender<Event>,
) -> Result<(ConnectionResult, Framed), Failure> {
    let mut retried = false;
    for _ in 0..MAX_HOPS {
        let result = match attempt(target, redirect.as_deref(), events).await {
            // A server can give up on a connection while its certificate
            // waits on the user; once trusted, a second try goes straight
            // through.
            Err(Attempt::AfterQuestion(_)) if !retried => {
                tracing::info!("retrying after the certificate question");
                retried = true;
                attempt(target, redirect.as_deref(), events).await
            }
            result => result,
        };
        match result {
            Ok((ConnectionOutcome::Connected(result), framed)) => return Ok((result, framed)),
            Ok((ConnectionOutcome::Redirected(pdu), _)) => {
                tracing::info!("the server redirected the connection");
                follow(target, &pdu)?;
                redirect = Some(pdu);
            }
            Err(Attempt::Failed(failure) | Attempt::AfterQuestion(failure)) => {
                return Err(failure);
            }
        }
    }
    Err(Failure::Protocol("too many redirections".into()))
}

/// Point `target` where a Server Redirection PDU says to go.
pub(super) fn follow(target: &mut Target, pdu: &ServerRedirectionPdu) -> Result<(), Failure> {
    let flags = pdu.flags;
    if !flags.contains(ServerRedirectionFlags::NO_REDIRECT) {
        let host = pdu
            .target_fqdn
            .as_deref()
            .filter(|_| flags.contains(ServerRedirectionFlags::TARGET_FQDN))
            .or(pdu
                .target_net_address
                .as_deref()
                .filter(|_| flags.contains(ServerRedirectionFlags::TARGET_NET_ADDRESS)));
        if let Some(host) = host
            .map(|h| h.trim_end_matches('\0'))
            .filter(|h| !h.is_empty())
        {
            target.host = host.to_owned();
        }
    }
    if let Some(username) = &pdu.username {
        target.username = username.trim_end_matches('\0').to_owned();
    }
    if let Some(domain) = &pdu.domain {
        target.domain = domain.trim_end_matches('\0').to_owned();
    }
    // An encrypted password is for RDSTLS to pass on as it is; a plain one,
    // in UTF-16, signs in to the target.
    if let Some(password) = pdu.password.as_deref().filter(|_| !rdstls::needed(pdu)) {
        target.password = utf16(password);
    }
    Ok(())
}

fn utf16(bytes: &[u8]) -> String {
    let (units, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = units.iter().map(|u| u16::from_le_bytes(*u)).collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_owned()
}

enum Attempt {
    Failed(Failure),
    /// Failed after asking the user about the certificate.
    AfterQuestion(Failure),
}

impl From<Failure> for Attempt {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

async fn attempt(
    target: &Target,
    redirect: Option<&ServerRedirectionPdu>,
    events: &mpsc::Sender<Event>,
) -> Result<(ConnectionOutcome, Framed), Attempt> {
    let stream = match tokio::time::timeout(
        TCP_TIMEOUT,
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return Err(Failure::Unreachable(Unreachable::from_io(&e)).into()),
        Err(_) => return Err(Failure::Unreachable(Unreachable::NoAnswer).into()),
    };
    let _ = stream.set_nodelay(true);
    let client_addr = stream
        .local_addr()
        .map_err(|e| Failure::Unreachable(Unreachable::from_io(&e)))?;
    let mut framed = TokioFramed::new(stream);

    let mut connector =
        ClientConnector::new(config(target), client_addr).with_static_channel(channels());
    if let Some(pdu) = redirect {
        connector = with_redirection(connector, pdu);
    }

    // A redirection with an encrypted password takes RDSTLS, which the
    // connector can't ask for: ask here, and let it take the reply as if it
    // had asked itself.
    let rdstls = redirect.filter(|pdu| rdstls::needed(pdu));
    let should_upgrade = match rdstls {
        Some(pdu) => {
            rdstls::send_request(&mut framed, pdu).await?;
            connector.state = ClientConnectorState::ConnectionInitiationWaitConfirm {
                requested_protocol: SecurityProtocol::RDSTLS,
            };
            ironrdp_tokio::connect_begin(&mut framed, &mut connector)
                .await
                .map_err(|e| Failure::opening(&e))?
        }
        None => ironrdp_tokio::connect_begin(&mut framed, &mut connector)
            .await
            .map_err(|e| Failure::opening(&e))?,
    };
    let (stream, leftover) = framed.into_inner();
    // The certificate is checked here, by fingerprint, before anything is
    // sent: servers on a LAN have self-signed certificates no authority
    // could vouch for.
    let (tls, certificate) = ironrdp_tls::upgrade_with_certificate_validation(
        stream,
        &target.host,
        ironrdp_tls::CertificateValidation::DangerouslyAcceptInvalidCertificate,
    )
    .await
    .map_err(|e| Failure::tls(&e))?;
    let mut tls = super::tap::Tap::new(tls);
    let der = certificate
        .to_der()
        .map_err(|e| Failure::Protocol(e.to_string()))?;
    let vouched = redirect
        .and_then(|pdu| pdu.target_certificate.as_ref())
        .and_then(|c| c.certificate_der())
        .is_some_and(|expected| expected == der.as_slice());
    if let Some(pdu) = redirect {
        tracing::info!(
            "following a redirection: flags {:?}, RDSTLS {}, certificate as announced {}",
            pdu.flags,
            rdstls.is_some(),
            vouched
        );
    }
    let mut asked = false;
    if !vouched {
        asked = ask_trust(target, &der, events).await?;
    }
    let fail = |failure: Failure| match failure {
        Failure::Refused | Failure::Untrusted => Attempt::Failed(failure),
        _ if asked => Attempt::AfterQuestion(failure),
        _ => Attempt::Failed(failure),
    };

    if let Some(pdu) = rdstls {
        rdstls::authenticate(&mut tls, &target.username, &target.domain, pdu)
            .await
            .map_err(fail)?;
    }
    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let tls: Box<dyn Stream> = Box::new(tls);
    let mut framed = TokioFramed::new_with_leftover(tls, leftover);
    let public_key = ironrdp_tls::extract_tls_server_public_key(&certificate)
        .ok_or_else(|| Failure::Protocol("the certificate has no public key".into()))?
        .to_owned();
    let server_name = ServerName::new(target.host.clone());

    if connector.should_perform_credssp() {
        credssp(
            &mut connector,
            &mut framed,
            server_name.clone(),
            public_key.clone(),
        )
        .await
        .map_err(|e| {
            tracing::warn!("NLA: {}", describe(&e));
            fail(Failure::Refused)
        })?;
    }
    let outcome = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoKdc,
        server_name,
        public_key,
        None,
    )
    .await
    .map_err(|e| fail(Failure::from_connector(&e)))?;
    Ok((outcome, framed))
}

/// Ask the interface whether to trust the certificate; `Ok(true)` when the
/// user had to answer, rather than the certificate being known.
async fn ask_trust(
    target: &Target,
    der: &[u8],
    events: &mpsc::Sender<Event>,
) -> Result<bool, Attempt> {
    let (reply, answer) = oneshot::channel();
    events
        .send(Event::Certificate {
            host: target.host.clone(),
            port: target.port,
            fingerprint: fingerprint(der),
            reply,
        })
        .await
        .map_err(|_| Failure::Untrusted)?;
    match answer.await.unwrap_or(Trust::Rejected) {
        Trust::Known => Ok(false),
        Trust::Accepted => Ok(true),
        Trust::Rejected => Err(Failure::Untrusted.into()),
    }
}

/// SHA-256 of the certificate, as `ab:cd:…`, the form GNOME's Remote
/// Desktop settings show it in.
pub(crate) fn fingerprint(der: &[u8]) -> String {
    let hex = gtk::glib::compute_checksum_for_data(gtk::glib::ChecksumType::Sha256, der)
        .map(|s| s.to_string())
        .unwrap_or_default();
    hex.as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(":")
}

/// NLA, run here rather than inside `connect_finalize` so that a failure
/// in it can be told apart: it means the credentials were refused.
async fn credssp(
    connector: &mut ClientConnector,
    framed: &mut Framed,
    server_name: ServerName,
    public_key: Vec<u8>,
) -> ConnectorResult<()> {
    let ClientConnectorState::Credssp { selected_protocol } = connector.state else {
        return Err(ironrdp_connector::general_err!("not at NLA"));
    };
    let domain = connector.config.domain.clone();
    let (sequence, request) = CredsspSequence::init(
        connector.config.credentials.clone(),
        domain.as_deref(),
        selected_protocol,
        server_name,
        public_key,
        None,
    )?;
    let mut buf = ironrdp_core::WriteBuf::new();
    ironrdp_tokio::perform_credssp(framed, &mut NoKdc, &mut buf, sequence, request).await?;
    connector.mark_credssp_as_done();
    Ok(())
}

/// The dynamic channels: the Graphics Pipeline, whose output lands in the
/// session's image by way of the session itself, and Display Control. Each
/// is made afresh whenever the server opens it: xrdp opens both again after
/// a resize.
fn channels() -> ironrdp_dvc::DrdynvcClient {
    struct Pictures;
    impl ironrdp_egfx::client::GraphicsPipelineHandler for Pictures {}
    ironrdp_dvc::DrdynvcClient::new()
        .with_dynamic_channel_factory(ironrdp_egfx::CHANNEL_NAME, || {
            ironrdp_egfx::client::GraphicsPipelineClient::new(Box::new(Pictures), None)
        })
        .with_dynamic_channel_factory(ironrdp_displaycontrol::CHANNEL_NAME, || {
            ironrdp_displaycontrol::client::DisplayControlClient::new(|_| Ok(Vec::new()))
        })
}

fn with_redirection(connector: ClientConnector, pdu: &ServerRedirectionPdu) -> ClientConnector {
    let mut connector = connector.with_cluster_data(gcc::ClientClusterData {
        flags: gcc::RedirectionFlags::REDIRECTION_SUPPORTED
            | gcc::RedirectionFlags::REDIRECTED_SESSION_FIELD_VALID,
        redirection_version: gcc::RedirectionVersion::V6,
        redirected_session_id: pdu.session_id,
    });
    if let Some(token) = rdstls::routing_token(pdu) {
        connector = connector.with_load_balance_info(token);
    }
    if let Some(password) = &pdu.password {
        connector = connector.with_redirection_password_cookie(password.clone());
    }
    connector
}

fn config(target: &Target) -> Config {
    let client_name = gtk::glib::host_name().chars().take(15).collect::<String>();
    Config {
        desktop_size: DesktopSize {
            width: target.width,
            height: target.height,
        },
        monitor_layout: None,
        desktop_scale_factor: target.scale,
        desktop_physical_size: None,
        desktop_orientation: None,
        // Of the three the protocol allows, the nearest.
        device_scale_factor: Some(match target.scale {
            ..125 => 100,
            125..165 => 140,
            _ => 180,
        }),
        monitor_extended_layout: None,
        relative_mouse_input: false,
        enable_tls: true,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        credentials: Credentials::UsernamePassword {
            username: target.username.clone(),
            password: target.password.clone(),
        },
        domain: Some(target.domain.clone()).filter(|d| !d.is_empty()),
        client_build: 0,
        client_name,
        keyboard_type: gcc::KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        // The server keeps its own layout: keys go as scancodes.
        keyboard_layout: 0,
        connection_type: gcc::ConnectionType::Lan,
        ime_file_name: String::new(),
        bitmap: Some(BitmapConfig {
            lossy_compression: true,
            color_depth: 32,
            codecs: client_codecs_capabilities(&[]).unwrap_or_default(),
        }),
        dig_product_id: String::new(),
        client_dir: String::new(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: RailSupportLevel::empty(),
        platform: MajorPlatformType::UNIX,
        hardware_id: None,
        request_data: None,
        // xrdp signs in with the account in the Client Info PDU only when
        // asked to; servers that did NLA already ignore it.
        autologon: !target.password.is_empty(),
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: None,
        enable_server_pointer: true,
        pointer_software_rendering: false,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: true,
    }
}

/// Kerberos would need a KDC; Mote signs in with NTLM, which needs none.
struct NoKdc;

impl NetworkClient for NoKdc {
    async fn send(&mut self, _: &NetworkRequest) -> ConnectorResult<Vec<u8>> {
        Err(ironrdp_connector::general_err!("Kerberos is not supported"))
    }
}
