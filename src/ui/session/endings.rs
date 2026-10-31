//! What the window says when a session ends: a title and a sentence that
//! names the likely cause on a Linux server and where to fix it.

use crate::i18n::{i18n, i18n_f};
use crate::rdp::{Disconnect, Ending, Failure, Security, Unreachable};

pub(super) enum Shown {
    /// Nothing: the window is closing.
    Nothing,
    /// Ask for the account again, saying why.
    SignIn(String),
    Page {
        title: String,
        description: String,
    },
}

pub(super) fn shown(ending: Ending, host: &str, was_connected: bool) -> Shown {
    let page = |title: String, description: String| Shown::Page { title, description };
    let could_not = || i18n("Could Not Connect");
    let named = |text: &str| i18n_f(text, &[("host", &host)]);
    match ending {
        Ending::Closed => Shown::Nothing,
        Ending::Failed(failure) => match failure {
            Failure::Refused => Shown::SignIn(i18n(
                "The user name or password was not accepted. GNOME Remote Desktop takes the ones set in its Remote Desktop settings, not the login password.",
            )),
            Failure::NotAllowed => page(
                could_not(),
                named("This account is not allowed to connect to {host}."),
            ),
            Failure::Untrusted => page(
                i18n("Not Connected"),
                i18n("The computer’s identity was not trusted."),
            ),
            Failure::Unreachable(why) => page(
                could_not(),
                match why {
                    Unreachable::NotFound => named("{host} could not be found."),
                    Unreachable::Refused => {
                        named("{host} is not accepting remote desktop connections.")
                    }
                    Unreachable::NoAnswer => named("{host} did not answer."),
                    Unreachable::Closed => named(
                        "{host} closed the connection at once. If it uses GNOME Remote Login, check that a user name and password are set, and restart its remote desktop service after changing them.",
                    ),
                    Unreachable::NotRdp => named(
                        "{host} answered, but not as a remote desktop server. Check the port.",
                    ),
                    Unreachable::Other(error) => {
                        tracing::warn!("connecting: {error}");
                        named("{host} could not be reached.")
                    }
                },
            ),
            Failure::Security(security) => page(
                could_not(),
                match security {
                    Security::NoCertificate => named(
                        "{host} has no certificate for secure connections. Set one up in its remote desktop settings.",
                    ),
                    Security::LegacyOnly => named(
                        "{host} only offers the old RDP security, which is not safe. On xrdp, check that xrdp.ini sets security_layer to tls or negotiate, and that its certificate and key files exist.",
                    ),
                    Security::OldTls => named(
                        "{host} only offers an outdated version of TLS. Update its remote desktop server, or allow TLS 1.2 or later.",
                    ),
                    Security::Other(code) => {
                        tracing::warn!("negotiation failed: {code}");
                        named("{host} and this computer have no security setting in common.")
                    }
                },
            ),
            Failure::Rejected => page(
                could_not(),
                named(
                    "{host} ended the connection as it began, without saying why. KRdp does this when the user name or password is wrong.",
                ),
            ),
            Failure::Lost(error) => {
                tracing::warn!("connection lost: {error}");
                page(
                    i18n("Disconnected"),
                    i18n("The connection to the computer was lost."),
                )
            }
            Failure::Protocol(error) => {
                tracing::warn!("session failed: {error}");
                let title = if was_connected {
                    i18n("Disconnected")
                } else {
                    could_not()
                };
                page(title, error)
            }
        },
        Ending::ServerClosed(Some(Disconnect::Graphics)) => page(
            could_not(),
            named(
                "{host} can only send its picture as H.264 video, which needs Cisco’s OpenH264 library on this computer. On Fedora, install the openh264 package.",
            ),
        ),
        Ending::ServerClosed(Some(Disconnect::FreshCredentials)) => {
            Shown::SignIn(i18n("The computer asked for the password again."))
        }
        Ending::ServerClosed(reason) => page(
            i18n("Disconnected"),
            match reason {
                Some(Disconnect::LoggedOff) => i18n("The remote session was signed out."),
                Some(Disconnect::Idle) => i18n("The session was idle for too long."),
                Some(Disconnect::LogonTimeout) => {
                    i18n("The login screen timed out before anyone signed in.")
                }
                Some(Disconnect::TakenOver) => {
                    named("Another connection to {host} took the session over.")
                }
                Some(Disconnect::Denied) => named("{host} turned the connection down."),
                Some(Disconnect::ByAdministrator) => named("Someone on {host} ended the session."),
                Some(Disconnect::Shutdown) => named("{host} is shutting down or restarting."),
                Some(Disconnect::Licensing(text) | Disconnect::Other(text)) => text,
                Some(Disconnect::FreshCredentials | Disconnect::Graphics) | None => {
                    i18n("The computer ended the session.")
                }
            },
        ),
    }
}
