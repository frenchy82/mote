//! RDSTLS ([MS-RDPBCGR] 5.4.5.3), the security GNOME Remote Login hands a
//! session over with: the Server Redirection PDU carries a password the
//! client can't read, encrypted for the target, and the client passes it on
//! inside TLS with the redirection's GUID. IronRDP's connector doesn't do
//! this yet, so the connection request and this exchange are done here and
//! the connector picks up after them.

use ironrdp_pdu::nego::{
    ConnectionConfirm, ConnectionRequest, ConnectionRequestWithOpaqueRoutingToken,
    OpaqueRoutingToken, RequestFlags, SecurityProtocol,
};
use ironrdp_pdu::rdp::server_redirection::{ServerRedirectionFlags, ServerRedirectionPdu};
use ironrdp_pdu::x224::X224;
use ironrdp_tokio::{FramedWrite as _, TokioFramed};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpStream;

use super::{Failure, describe};

const VERSION_1: u16 = 0x0001;
const TYPE_CAPABILITIES: u16 = 0x0001;
const TYPE_AUTHREQ: u16 = 0x0002;
const TYPE_AUTHRSP: u16 = 0x0004;
const DATA_PASSWORD_CREDS: u16 = 0x0001;

/// Whether following `pdu` takes RDSTLS: its password is encrypted for the
/// target.
pub(super) fn needed(pdu: &ServerRedirectionPdu) -> bool {
    pdu.password.is_some()
        && pdu
            .flags
            .contains(ServerRedirectionFlags::PASSWORD_IS_PK_ENCRYPTED)
}

/// The routing token to send back, without the line end the X.224 request
/// adds again.
pub(super) fn routing_token(pdu: &ServerRedirectionPdu) -> Option<String> {
    let info = pdu.load_balance_info.as_ref()?;
    Some(
        String::from_utf8_lossy(info)
            .trim_end_matches("\r\n")
            .to_owned(),
    )
}

/// Ask for RDSTLS in the X.224 connection request, with the routing token.
pub(super) async fn request(
    framed: &mut TokioFramed<TcpStream>,
    pdu: &ServerRedirectionPdu,
) -> Result<(), Failure> {
    let request = ConnectionRequestWithOpaqueRoutingToken {
        request: ConnectionRequest {
            nego_data: None,
            flags: RequestFlags::empty(),
            protocol: SecurityProtocol::RDSTLS,
            correlation_info: None,
        },
        routing_token: OpaqueRoutingToken(routing_token(pdu).unwrap_or_default()),
    };
    let bytes =
        ironrdp_core::encode_vec(&X224(request)).map_err(|e| Failure::Protocol(describe(&e)))?;
    framed
        .write_all(&bytes)
        .await
        .map_err(|e| Failure::Lost(describe(&e)))?;
    let reply = framed
        .read_by_hint(&ironrdp_pdu::X224_HINT)
        .await
        .map_err(|e| Failure::Lost(describe(&e)))?;
    match ironrdp_core::decode::<X224<ConnectionConfirm>>(&reply)
        .map_err(|e| Failure::Protocol(describe(&e)))?
        .0
    {
        ConnectionConfirm::Response { protocol, .. }
            if protocol.contains(SecurityProtocol::RDSTLS) =>
        {
            Ok(())
        }
        other => Err(Failure::Protocol(format!(
            "the handover target turned RDSTLS down: {other:?}"
        ))),
    }
}

/// The exchange inside TLS: the server's capabilities, the credentials from
/// the redirection, and the server's verdict.
pub(super) async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    username: &str,
    domain: &str,
    pdu: &ServerRedirectionPdu,
) -> Result<(), Failure> {
    let lost = |e: std::io::Error| Failure::Lost(describe(&e));
    let mut capabilities = [0u8; 8];
    stream.read_exact(&mut capabilities).await.map_err(lost)?;
    if u16::from_le_bytes([capabilities[2], capabilities[3]]) != TYPE_CAPABILITIES {
        return Err(Failure::Protocol(
            "RDSTLS: no capabilities from the server".into(),
        ));
    }
    stream
        .write_all(&request_bytes(username, domain, pdu))
        .await
        .map_err(lost)?;
    stream.flush().await.map_err(lost)?;
    let mut response = [0u8; 10];
    stream.read_exact(&mut response).await.map_err(lost)?;
    let kind = u16::from_le_bytes([response[2], response[3]]);
    let code = u32::from_le_bytes([response[6], response[7], response[8], response[9]]);
    match (kind, code) {
        (TYPE_AUTHRSP, 0) => Ok(()),
        (TYPE_AUTHRSP, _) => {
            tracing::warn!("RDSTLS: the handover was refused with {code:#x}");
            Err(Failure::Refused)
        }
        _ => Err(Failure::Protocol(format!(
            "RDSTLS: unexpected reply type {kind:#x}"
        ))),
    }
}

/// The Authentication Request with Password Credentials, laid out as
/// FreeRDP sends it.
fn request_bytes(username: &str, domain: &str, pdu: &ServerRedirectionPdu) -> Vec<u8> {
    let mut out = Vec::new();
    for word in [VERSION_1, TYPE_AUTHREQ, DATA_PASSWORD_CREDS] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    data(
        &mut out,
        pdu.redirection_guid.as_deref().unwrap_or_default(),
    );
    string(&mut out, username);
    string(&mut out, domain);
    data(&mut out, pdu.password.as_deref().unwrap_or_default());
    out
}

fn data(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// UTF-16 with its terminating null, the length counting both.
fn string(out: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    out.extend_from_slice(&((units.len() * 2) as u16).to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_the_password_request() {
        let pdu = ServerRedirectionPdu {
            redirection_guid: Some(vec![0xAA, 0xBB]),
            password: Some(vec![1, 2, 3]),
            ..Default::default()
        };
        let bytes = request_bytes("u", "", &pdu);
        assert_eq!(
            bytes,
            [
                1, 0, 2, 0, 1, 0, // version, type, data type
                2, 0, 0xAA, 0xBB, // GUID
                4, 0, b'u', 0, 0, 0, // "u" and its null
                2, 0, 0, 0, // an empty domain: the null alone
                3, 0, 1, 2, 3, // the password blob, as received
            ]
        );
    }

    #[test]
    fn needs_rdstls_only_for_an_encrypted_password() {
        let mut pdu = ServerRedirectionPdu {
            password: Some(vec![1]),
            ..Default::default()
        };
        assert!(!needed(&pdu));
        pdu.flags = ServerRedirectionFlags::PASSWORD_IS_PK_ENCRYPTED;
        assert!(needed(&pdu));
    }
}
