//! How a session can end, sorted so the interface can say why in plain
//! words and what to do about it. The causes are the ones Linux servers
//! meet: GNOME Remote Desktop without credentials, xrdp set to old
//! security, a server without a certificate, someone else taking over a
//! shared desktop.

use ironrdp_connector::{ConnectorError, ConnectorErrorKind};
use ironrdp_pdu::nego::FailureCode;
use ironrdp_pdu::rdp::server_error_info::{
    ErrorInfo, ProtocolIndependentCode, ProtocolIndependentLicensingCode, RdpSpecificCode,
};

use super::{describe, io_kind};

pub(crate) enum Ending {
    /// Closed from this side.
    Closed,
    /// The server ended the session, with its reason when it gave one.
    ServerClosed(Option<Disconnect>),
    Failed(Failure),
}

pub(crate) enum Failure {
    /// The computer couldn't be reached.
    Unreachable(Unreachable),
    /// The user name or password was refused.
    Refused,
    /// The account may not connect, whatever its password.
    NotAllowed,
    /// The certificate wasn't trusted.
    Untrusted,
    /// No security both sides accept.
    Security(Security),
    /// The server ended the connection as it began, giving no reason.
    Rejected,
    /// The connection dropped.
    Lost(String),
    /// Anything else, as the engine describes it.
    Protocol(String),
}

/// Why a computer couldn't be reached.
pub(crate) enum Unreachable {
    /// The name doesn't resolve.
    NotFound,
    /// Nothing listens on the port.
    Refused,
    NoAnswer,
    /// It took the connection and closed it before anything began, as GNOME
    /// Remote Desktop does when it has no credentials.
    Closed,
    /// Something answered, but not in RDP.
    NotRdp,
    Other(String),
}

pub(crate) enum Security {
    /// The server has no certificate to offer TLS with.
    NoCertificate,
    /// The server offers only Standard RDP Security, which isn't safe.
    LegacyOnly,
    /// The server's TLS is older than 1.2.
    OldTls,
    Other(String),
}

/// The reason a server gave for ending a session ([MS-RDPBCGR] 2.2.5.1.1).
pub(crate) enum Disconnect {
    LoggedOff,
    Idle,
    LogonTimeout,
    /// Another connection took the session over.
    TakenOver,
    Denied,
    /// Ended by someone on the server.
    ByAdministrator,
    Shutdown,
    /// The server wants the credentials typed again.
    FreshCredentials,
    /// The server can't send its picture in a form this client decodes:
    /// KRdp, which needs H.264.
    Graphics,
    Licensing(String),
    Other(String),
}

impl Unreachable {
    pub(super) fn from_io(error: &std::io::Error) -> Self {
        use std::io::ErrorKind;
        match error.kind() {
            ErrorKind::ConnectionRefused => Self::Refused,
            ErrorKind::TimedOut | ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
                Self::NoAnswer
            }
            // The resolver's errors have no kind of their own.
            _ if error.to_string().contains("lookup") => Self::NotFound,
            _ => Self::Other(error.to_string()),
        }
    }
}

impl Failure {
    /// A failure in the first exchange, before TLS: a server that closes
    /// the connection then turns every client away.
    pub(super) fn opening(error: &ConnectorError) -> Self {
        use std::io::ErrorKind;
        match error.kind() {
            ConnectorErrorKind::Negotiation(failure) => Self::Security(security(failure.code())),
            // xrdp falls back to Standard RDP Security, rather than refusing
            // as the spec has it, when set to it or when its certificate is
            // missing; IronRDP says so only in its message.
            ConnectorErrorKind::Reason(text) if text.contains("STANDARD_RDP_SECURITY") => {
                Self::Security(Security::LegacyOnly)
            }
            // What came back wasn't an X.224 Connection Confirm.
            ConnectorErrorKind::Decode(_) => Self::Unreachable(Unreachable::NotRdp),
            // Not even framed as RDP: an SSH or VNC port, say.
            _ if undecodable(error) => Self::Unreachable(Unreachable::NotRdp),
            _ => match io_kind(error) {
                Some(
                    ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe,
                ) => {
                    tracing::warn!("opening: {}", describe(error));
                    Self::Unreachable(Unreachable::Closed)
                }
                _ => Self::Protocol(describe(error)),
            },
        }
    }

    /// A failed TLS handshake.
    pub(super) fn tls(error: &std::io::Error) -> Self {
        use rustls::{AlertDescription, Error};
        let tls = error.get_ref().and_then(|e| e.downcast_ref::<Error>());
        match tls {
            Some(
                Error::PeerIncompatible(_)
                | Error::AlertReceived(
                    AlertDescription::ProtocolVersion
                    | AlertDescription::HandshakeFailure
                    | AlertDescription::InsufficientSecurity,
                ),
            ) => {
                tracing::warn!("TLS: {}", describe(error));
                Self::Security(Security::OldTls)
            }
            _ => match error.kind() {
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof => {
                    tracing::warn!("TLS: {}", describe(error));
                    Self::Unreachable(Unreachable::Closed)
                }
                _ => Self::Lost(describe(error)),
            },
        }
    }

    /// A failure after TLS, while the connection is set up.
    pub(super) fn from_connector(error: &ConnectorError) -> Self {
        use std::io::ErrorKind;
        match error.kind() {
            ConnectorErrorKind::AccessDenied => Self::NotAllowed,
            ConnectorErrorKind::Credssp(_) => Self::Refused,
            ConnectorErrorKind::Negotiation(failure) => Self::Security(security(failure.code())),
            _ => {
                let text = describe(error);
                tracing::warn!("setting up: {text}");
                let closed = matches!(
                    io_kind(error),
                    Some(
                        ErrorKind::ConnectionReset
                            | ErrorKind::UnexpectedEof
                            | ErrorKind::BrokenPipe
                    )
                );
                // IronRDP reports a Disconnect Provider Ultimatum only in its
                // message.
                if closed || text.contains("disconnect provider ultimatum") {
                    Self::Rejected
                } else {
                    Self::Protocol(text)
                }
            }
        }
    }
}

/// Whether an error comes from bytes that didn't decode as RDP. The framing
/// wraps its decode errors in I/O errors, whose own chain skips them.
fn undecodable(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(e) = current {
        if e.is::<ironrdp_core::DecodeError>() {
            return true;
        }
        if let Some(inner) = e
            .downcast_ref::<std::io::Error>()
            .and_then(|io| io.get_ref())
            && inner.is::<ironrdp_core::DecodeError>()
        {
            return true;
        }
        current = e.source();
    }
    false
}

fn security(code: FailureCode) -> Security {
    if code == FailureCode::SSL_CERT_NOT_ON_SERVER {
        Security::NoCertificate
    } else if code == FailureCode::SSL_NOT_ALLOWED_BY_SERVER {
        Security::LegacyOnly
    } else {
        Security::Other(format!("{code:?}"))
    }
}

impl Disconnect {
    /// The reason in a Set Error Info PDU; `None` for "no error".
    pub(super) fn from_error_info(info: ErrorInfo) -> Option<Self> {
        use ProtocolIndependentCode as Code;
        Some(match info {
            ErrorInfo::ProtocolIndependentCode(code) => match code {
                Code::None => return None,
                Code::RpcInitiatedLogoff | Code::LogoffByUser => Self::LoggedOff,
                Code::IdleTimeout => Self::Idle,
                Code::LogonTimeout => Self::LogonTimeout,
                Code::DisconnectedByOtherconnection => Self::TakenOver,
                Code::ServerDeniedConnection | Code::ServerInsufficientPrivileges => Self::Denied,
                Code::RpcInitiatedDisconnect | Code::RpcInitiatedDisconnectByuser => {
                    Self::ByAdministrator
                }
                Code::ServerShutdown | Code::ServerReboot => Self::Shutdown,
                Code::ServerFreshCredentialsRequired => Self::FreshCredentials,
                other => Self::Other(other.description().to_owned()),
            },
            ErrorInfo::ProtocolIndependentLicensingCode(code) => {
                if code == ProtocolIndependentLicensingCode::NoRemoteConnections {
                    Self::Denied
                } else {
                    Self::Licensing(code.description().to_owned())
                }
            }
            ErrorInfo::RdpSpecificCode(RdpSpecificCode::GraphicsSubsystemFailed) => Self::Graphics,
            other => Self::Other(other.description()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_reasons_servers_give() {
        let reason = |code| Disconnect::from_error_info(ErrorInfo::ProtocolIndependentCode(code));
        assert!(reason(ProtocolIndependentCode::None).is_none());
        assert!(matches!(
            reason(ProtocolIndependentCode::DisconnectedByOtherconnection),
            Some(Disconnect::TakenOver)
        ));
        assert!(matches!(
            reason(ProtocolIndependentCode::LogoffByUser),
            Some(Disconnect::LoggedOff)
        ));
        assert!(matches!(
            reason(ProtocolIndependentCode::ServerReboot),
            Some(Disconnect::Shutdown)
        ));
    }

    #[test]
    fn reads_negotiation_failures() {
        assert!(matches!(
            security(FailureCode::SSL_CERT_NOT_ON_SERVER),
            Security::NoCertificate
        ));
        assert!(matches!(
            security(FailureCode::SSL_NOT_ALLOWED_BY_SERVER),
            Security::LegacyOnly
        ));
    }
}
