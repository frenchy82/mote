//! The RDP engine: one session's connection, run on the tokio runtime, away
//! from the GTK main thread. The interface starts it with [`start`], talks
//! to it with [`Command`]s and hears back through [`Event`]s; nothing here
//! touches a widget.

mod connect;
pub(crate) mod keymap;
mod session;

use std::sync::Arc;

use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_pdu::input::fast_path::FastPathInputEvent;
use tokio::sync::{mpsc, oneshot};

/// Where to connect, and as whom.
#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) domain: String,
    /// The desktop size to ask for, in pixels.
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub(crate) enum Event {
    /// The server's certificate, checked before any credentials go out.
    Certificate {
        host: String,
        port: u16,
        fingerprint: String,
        reply: oneshot::Sender<Trust>,
    },
    /// The session is up, or the server changed the desktop's size.
    Connected {
        width: u16,
        height: u16,
    },
    /// A new picture of the desktop. The next one comes after
    /// [`Command::FrameDone`].
    Frame(Frame),
    Cursor(Cursor),
    /// The last event of a session.
    Ended(Ending),
}

/// The whole desktop in BGRX, and the parts of it that changed since the
/// last frame.
pub(crate) struct Frame {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) stride: usize,
    pub(crate) pixels: Vec<u8>,
    pub(crate) damage: Vec<Rect>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// The answer to [`Event::Certificate`].
pub(crate) enum Trust {
    /// Trusted before.
    Known,
    /// Trusted just now, by the user.
    Accepted,
    Rejected,
}

pub(crate) enum Cursor {
    Default,
    Hidden,
    /// RGBA, not premultiplied.
    Bitmap(Arc<DecodedPointer>),
}

pub(crate) enum Ending {
    /// Closed from this side.
    Closed,
    /// The server ended the session, with its reason when it gave one.
    ServerClosed(Option<String>),
    Failed(Failure),
}

pub(crate) enum Failure {
    /// The computer couldn't be reached.
    Unreachable(Unreachable),
    /// The user name or password was refused.
    Refused,
    /// The certificate wasn't trusted.
    Untrusted,
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
    Other(String),
}

impl Unreachable {
    fn from_io(error: &std::io::Error) -> Self {
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

pub(crate) enum Command {
    Input(Vec<FastPathInputEvent>),
    /// The last frame is on screen; the next can come.
    FrameDone,
    Close,
}

/// A running session. It ends on [`Command::Close`], or once every copy of
/// its handle is gone.
#[derive(Clone)]
pub(crate) struct Handle {
    commands: mpsc::UnboundedSender<Command>,
}

impl Handle {
    pub(crate) fn send(&self, command: Command) {
        // A session that has ended has nobody to hear it.
        let _ = self.commands.send(command);
    }
}

/// Connect to `target`, and keep the session going until it ends.
pub(crate) fn start(target: Target) -> (Handle, mpsc::Receiver<Event>) {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (event_tx, events) = mpsc::channel(8);
    // A thread of its own per session, on a single-threaded runtime: the
    // NLA exchange holds state that can't move between threads, and one
    // busy session can't slow another.
    let spawned = std::thread::Builder::new()
        .name("rdp-session".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(run(target, event_tx, command_rx)),
                Err(e) => {
                    let failure = Failure::Protocol(e.to_string());
                    let _ = event_tx.blocking_send(Event::Ended(Ending::Failed(failure)));
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!("starting a session thread: {e}");
    }
    (Handle { commands }, events)
}

/// How many redirections one connection follows before giving up.
const MAX_HOPS: usize = 4;

async fn run(
    mut target: Target,
    events: mpsc::Sender<Event>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let ending = match drive(&mut target, &events, &mut commands).await {
        Ok(ending) => ending,
        Err(failure) => Ending::Failed(failure),
    };
    let _ = events.send(Event::Ended(ending)).await;
}

async fn drive(
    target: &mut Target,
    events: &mpsc::Sender<Event>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Result<Ending, Failure> {
    let mut redirect = None;
    for _ in 0..MAX_HOPS {
        let (result, framed) = tokio::select! {
            connected = connect::connect(target, redirect.take(), events) => connected?,
            () = closed(commands) => return Ok(Ending::Closed),
        };
        match session::run(framed, result, events, commands).await? {
            session::Outcome::Ended(ending) => return Ok(ending),
            // A server can hand the session on after it starts: GNOME's
            // Remote Login does, once someone logs in at its login screen.
            session::Outcome::Redirect(pdu) => {
                tracing::info!("the server redirected the session");
                connect::follow(target, &pdu)?;
                redirect = Some(pdu);
            }
        }
    }
    Err(Failure::Protocol("too many redirections".into()))
}

/// Resolves when the interface closes the session, ignoring input sent
/// before there is a session to take it.
async fn closed(commands: &mut mpsc::UnboundedReceiver<Command>) {
    while let Some(command) = commands.recv().await {
        if matches!(command, Command::Close) {
            return;
        }
    }
}

/// An error and its causes, on one line.
fn describe(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connects to the server in `MOTE_TEST_SERVER` (`host:port`, user and
    /// password `mote`), as `temp/grd/run.sh` starts one:
    /// `MOTE_TEST_SERVER=localhost:3399 cargo test -- --ignored live`.
    /// With `MOTE_TEST_PASSWORD` set to another password, it expects to be
    /// refused.
    #[test]
    #[ignore]
    fn live() {
        let server = std::env::var("MOTE_TEST_SERVER").expect("MOTE_TEST_SERVER");
        let (host, port) = server.rsplit_once(':').expect("host:port");
        let target = Target {
            host: host.into(),
            port: port.parse().expect("port"),
            username: "mote".into(),
            password: std::env::var("MOTE_TEST_PASSWORD").unwrap_or("mote".into()),
            domain: String::new(),
            width: 1024,
            height: 768,
        };
        let (handle, mut events) = start(target);
        let mut frames = 0;
        while let Some(event) = events.blocking_recv() {
            match event {
                Event::Certificate {
                    fingerprint, reply, ..
                } => {
                    eprintln!("certificate {fingerprint}");
                    let _ = reply.send(Trust::Accepted);
                }
                Event::Connected { width, height } => eprintln!("connected {width}x{height}"),
                Event::Frame(frame) => {
                    frames += 1;
                    eprintln!("frame {} damage {:?}", frames, frame.damage.first());
                    // An idle desktop sends nothing more.
                    handle.send(Command::Close);
                }
                Event::Cursor(_) => eprintln!("cursor"),
                Event::Ended(ending) => {
                    let text = match ending {
                        Ending::Closed => "closed".to_owned(),
                        Ending::ServerClosed(r) => format!("server closed {r:?}"),
                        Ending::Failed(Failure::Refused) => "refused".to_owned(),
                        Ending::Failed(Failure::Untrusted) => "untrusted".to_owned(),
                        Ending::Failed(Failure::Unreachable(_)) => "unreachable".to_owned(),
                        Ending::Failed(Failure::Lost(e) | Failure::Protocol(e)) => {
                            format!("failed: {e}")
                        }
                    };
                    eprintln!("ended: {text}");
                    if std::env::var("MOTE_TEST_PASSWORD").is_ok_and(|p| p != "mote") {
                        assert_eq!(text, "refused");
                        return;
                    }
                    assert_eq!(text, "closed");
                    break;
                }
            }
        }
        assert_eq!(frames, 1);
    }
}
