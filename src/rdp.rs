//! The RDP engine: one session's connection, run on the tokio runtime, away
//! from the GTK main thread. The interface starts it with [`start`], talks
//! to it with [`Command`]s and hears back through [`Event`]s; nothing here
//! touches a widget.

mod compat;
mod connect;
mod failure;
pub(crate) mod keymap;
mod rdstls;
mod session;
mod tap;

use std::sync::Arc;

use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_pdu::input::fast_path::FastPathInputEvent;
use tokio::sync::{mpsc, oneshot};

pub(crate) use failure::{Disconnect, Ending, Failure, Security, Unreachable};

/// Where to connect, and as whom.
#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) domain: String,
    /// The desktop size to ask for, in the screen's own pixels.
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// The screen's scale, in percent: 200 on a 2× screen.
    pub(crate) scale: u32,
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

pub(crate) enum Command {
    Input(Vec<FastPathInputEvent>),
    /// Make the remote desktop this size, in the screen's own pixels, at
    /// this scale in percent.
    Resize {
        width: u32,
        height: u32,
        scale: u32,
    },
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
        let opened = (
            u32::from(target.width),
            u32::from(target.height),
            target.scale,
        );
        match session::run(framed, result, events, commands, opened).await? {
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

/// The physical size, in millimeters, of a screen with this many pixels at
/// this scale: the size at which 96 DPI times the scale comes out. X11
/// desktops behind xrdp take their DPI from it, as they ignore the scale.
fn physical_size(width: u32, height: u32, scale: u32) -> (u32, u32) {
    let millimeters =
        |pixels: u32| (u64::from(pixels) * 2540 / (96 * u64::from(scale.max(100)))) as u32;
    (millimeters(width), millimeters(height))
}

/// The I/O error at the root of an error, if there is one.
fn io_kind(error: &(dyn std::error::Error + 'static)) -> Option<std::io::ErrorKind> {
    let mut current = Some(error);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return Some(io.kind());
        }
        current = e.source();
    }
    None
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

    #[test]
    fn physical_size_carries_the_scale() {
        // 96 DPI at 100%: 960 pixels are 254 mm; at 200% the same pixels
        // are half that.
        assert_eq!(physical_size(960, 960, 100), (254, 254));
        assert_eq!(physical_size(960, 960, 200), (127, 127));
    }

    /// The frame as a PPM picture, for `MOTE_TEST_PICTURE=<file.ppm>`.
    fn save_ppm(path: &str, frame: &Frame) {
        let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        for row in frame.pixels.chunks(frame.stride) {
            for bgrx in row[..usize::from(frame.width) * 4].chunks(4) {
                out.extend_from_slice(&[bgrx[2], bgrx[1], bgrx[0]]);
            }
        }
        std::fs::write(path, out).expect("writes the picture");
    }

    /// Connects to the server in `MOTE_TEST_SERVER` (`host:port`) as
    /// `MOTE_TEST_USER` with `MOTE_TEST_PASSWORD` (both `mote` by default,
    /// as `temp/grd/run.sh` sets them up), and expects the session to end
    /// as `MOTE_TEST_EXPECT` says: `closed` (by default) after the first
    /// frame, or `refused`. `MOTE_LOG` sets the log filter.
    /// `MOTE_TEST_SERVER=localhost:3399 cargo test -- --ignored live`.
    #[test]
    #[ignore]
    fn live() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_env("MOTE_LOG")
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mote=info")),
            )
            .with_test_writer()
            .try_init();
        let var = |name: &str, default: &str| std::env::var(name).unwrap_or(default.into());
        let server = std::env::var("MOTE_TEST_SERVER").expect("MOTE_TEST_SERVER");
        let (host, port) = server.rsplit_once(':').expect("host:port");
        let target = Target {
            host: host.into(),
            port: port.parse().expect("port"),
            username: var("MOTE_TEST_USER", "mote"),
            password: var("MOTE_TEST_PASSWORD", "mote"),
            domain: String::new(),
            width: 1024,
            height: 768,
            scale: 100,
        };
        let (handle, mut events) = start(target);
        // MOTE_TEST_SECONDS: stay connected that long, for a desktop that
        // takes a while to draw itself.
        let stay = std::env::var("MOTE_TEST_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
        if let Some(seconds) = stay {
            let closer = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(seconds));
                closer.send(Command::Close);
            });
        }
        let mut frames = 0;
        let mut last_size = (0, 0);
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
                    if let Ok(path) = std::env::var("MOTE_TEST_PICTURE") {
                        save_ppm(&path, &frame);
                    }
                    if frames == 1 {
                        eprintln!("frame 1 damage {:?}", frame.damage.first());
                        // MOTE_TEST_RESIZE=<width>x<height>@<scale>: ask for
                        // that size after the first frame.
                        if let Some((width, height, scale)) =
                            std::env::var("MOTE_TEST_RESIZE").ok().and_then(|s| {
                                let (size, scale) = s.split_once('@')?;
                                let (w, h) = size.split_once('x')?;
                                Some((w.parse().ok()?, h.parse().ok()?, scale.parse().ok()?))
                            })
                        {
                            handle.send(Command::Resize {
                                width,
                                height,
                                scale,
                            });
                        }
                    }
                    if frame.width != last_size.0 || frame.height != last_size.1 {
                        eprintln!("frame size {}x{}", frame.width, frame.height);
                        last_size = (frame.width, frame.height);
                    }
                    if stay.is_some() {
                        handle.send(Command::FrameDone);
                    } else {
                        // An idle desktop sends nothing more.
                        handle.send(Command::Close);
                    }
                }
                Event::Cursor(_) => eprintln!("cursor"),
                Event::Ended(ending) => {
                    let text = match ending {
                        Ending::Closed => "closed".to_owned(),
                        Ending::ServerClosed(Some(Disconnect::Graphics)) => "needs h264".to_owned(),
                        Ending::ServerClosed(_) => "server closed".to_owned(),
                        Ending::Failed(Failure::Refused) => "refused".to_owned(),
                        Ending::Failed(Failure::Untrusted) => "untrusted".to_owned(),
                        Ending::Failed(Failure::Unreachable(why)) => match why {
                            Unreachable::NotFound => "not found",
                            Unreachable::Refused => "connection refused",
                            Unreachable::NoAnswer => "no answer",
                            Unreachable::Closed => "closed at once",
                            Unreachable::NotRdp => "not rdp",
                            Unreachable::Other(_) => "unreachable",
                        }
                        .to_owned(),
                        Ending::Failed(Failure::Lost(e) | Failure::Protocol(e)) => {
                            format!("failed: {e}")
                        }
                        Ending::Failed(Failure::NotAllowed) => "not allowed".to_owned(),
                        Ending::Failed(Failure::Security(security)) => match security {
                            Security::NoCertificate => "no certificate",
                            Security::LegacyOnly => "legacy only",
                            Security::OldTls => "old tls",
                            Security::Other(_) => "security",
                        }
                        .to_owned(),
                        Ending::Failed(Failure::Rejected) => "rejected".to_owned(),
                    };
                    eprintln!("ended: {text}");
                    let expected = var("MOTE_TEST_EXPECT", "closed");
                    assert_eq!(text, expected);
                    if expected != "closed" {
                        return;
                    }
                    break;
                }
            }
        }
        eprintln!("frames {frames}");
        assert!(frames >= 1);
    }
}
