//! A running session: frames in, input out, until one side ends it.

use ironrdp_connector::ConnectionResult;
use ironrdp_connector::Sequence as _;
use ironrdp_connector::connection_activation::ConnectionActivationState;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::geometry::{InclusiveRectangle, Rectangle as _};
use ironrdp_pdu::rdp::server_redirection::ServerRedirectionPdu;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{
    ActiveStage, ActiveStageBuilder, ActiveStageOutput, GracefulDisconnectReason,
};
use ironrdp_tokio::{FramedWrite as _, split_tokio_framed};
use tokio::sync::mpsc;

use super::connect::Framed;
use super::{Command, Cursor, Disconnect, Ending, Event, Failure, Frame, Rect, describe};

pub(super) enum Outcome {
    Ended(Ending),
    /// The server handed the session on; connect again where it says.
    Redirect(Box<ServerRedirectionPdu>),
}

/// What the loop holds between frames.
struct Desktop {
    image: DecodedImage,
    /// Changed since the last frame went out.
    damage: Vec<Rect>,
    /// A frame is with the interface, not yet drawn.
    in_flight: bool,
    /// A frame has gone out.
    shown: bool,
}

impl Desktop {
    fn new(width: u16, height: u16) -> Self {
        Self {
            // BGRX is what GDK takes without converting.
            image: DecodedImage::new(PixelFormat::BgrX32, width, height),
            damage: Vec::new(),
            in_flight: false,
            shown: false,
        }
    }
}

pub(super) async fn run(
    framed: Framed,
    result: ConnectionResult,
    events: &mpsc::Sender<Event>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Result<Outcome, Failure> {
    let (mut reader, mut writer) = split_tokio_framed(framed);
    let size = result.desktop_size;
    let mut desktop = Desktop::new(size.width, size.height);
    send(
        events,
        Event::Connected {
            width: size.width,
            height: size.height,
        },
    )
    .await?;
    let activation = result.activation_factory;
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    // The reason the server gave, ahead of ending the session.
    let mut reason = None;
    loop {
        let outputs = tokio::select! {
            frame = reader.read_pdu() => {
                let (action, mut payload) = match frame {
                    Ok(frame) => frame,
                    // A server that said why can close without more ado.
                    Err(_) if reason.is_some() => {
                        return Ok(Outcome::Ended(Ending::ServerClosed(reason)));
                    }
                    Err(e) => return Err(Failure::Lost(describe(&e))),
                };
                if action == ironrdp_pdu::Action::X224
                    && let Some(info) = super::compat::error_info(&payload)
                {
                    reason = Disconnect::from_error_info(info).or(reason);
                }
                if action == ironrdp_pdu::Action::X224 && super::compat::normalize_redirection(&mut payload) {
                    tracing::debug!("took a FreeRDP-style redirection header");
                }
                stage
                    .process(&mut desktop.image, action, &payload, reader.last_read_at())
                    .map_err(|e| failed(&e, desktop.shown))?
            }
            command = commands.recv() => match command {
                Some(Command::Input(input)) => stage
                    .process_fastpath_input(&mut desktop.image, &input)
                    .map_err(|e| Failure::Protocol(describe(&e)))?,
                Some(Command::FrameDone) => {
                    desktop.in_flight = false;
                    Vec::new()
                }
                Some(Command::Close) | None => {
                    // Ask the server to end the session, without waiting
                    // for it to agree: the window is already gone.
                    if let Ok(outputs) = stage.graceful_shutdown() {
                        for output in outputs {
                            if let ActiveStageOutput::ResponseFrame(frame) = output {
                                let _ = writer.write_all(&frame).await;
                            }
                        }
                    }
                    return Ok(Outcome::Ended(Ending::Closed));
                }
            },
        };

        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(frame) => {
                    if !frame.is_empty() {
                        writer
                            .write_all(&frame)
                            .await
                            .map_err(|e| Failure::Lost(describe(&e)))?;
                    }
                }
                ActiveStageOutput::GraphicsUpdate(region) => desktop.damage.push(rect(&region)),
                ActiveStageOutput::PointerDefault => {
                    send(events, Event::Cursor(Cursor::Default)).await?;
                }
                ActiveStageOutput::PointerHidden => {
                    send(events, Event::Cursor(Cursor::Hidden)).await?;
                }
                ActiveStageOutput::PointerBitmap(pointer) => {
                    send(events, Event::Cursor(Cursor::Bitmap(pointer))).await?;
                }
                ActiveStageOutput::DeactivateAll => {
                    if let Some(reason) = reactivate(
                        &mut stage,
                        &activation,
                        &mut reader,
                        &mut writer,
                        &mut desktop,
                    )
                    .await?
                    {
                        return Ok(Outcome::Ended(Ending::ServerClosed(Some(reason))));
                    }
                    send(
                        events,
                        Event::Connected {
                            width: desktop.image.width(),
                            height: desktop.image.height(),
                        },
                    )
                    .await?;
                }
                ActiveStageOutput::Terminate(why) => {
                    let said = match why {
                        GracefulDisconnectReason::Other(text) => Some(Disconnect::Other(text)),
                        _ => None,
                    };
                    return Ok(Outcome::Ended(Ending::ServerClosed(reason.or(said))));
                }
                ActiveStageOutput::ServerRedirection(pdu) => return Ok(Outcome::Redirect(pdu)),
                _ => {}
            }
        }

        if !desktop.damage.is_empty() && !desktop.in_flight {
            let image = &desktop.image;
            let frame = Frame {
                width: image.width(),
                height: image.height(),
                stride: image.stride(),
                pixels: image.data().to_vec(),
                damage: std::mem::take(&mut desktop.damage),
            };
            desktop.in_flight = true;
            desktop.shown = true;
            send(events, Event::Frame(frame)).await?;
        }
    }
}

/// The Deactivation-Reactivation Sequence: the server renegotiates, often
/// with a new desktop size. `Ok(Some(reason))` when the server ends the
/// session instead, as KRdp does when the client can't decode H.264.
async fn reactivate(
    stage: &mut ActiveStage,
    activation: &ironrdp_connector::connection_activation::ConnectionActivationFactory,
    reader: &mut ironrdp_tokio::TokioFramed<tokio::io::ReadHalf<Box<dyn super::connect::Stream>>>,
    writer: &mut ironrdp_tokio::TokioFramed<tokio::io::WriteHalf<Box<dyn super::connect::Stream>>>,
    desktop: &mut Desktop,
) -> Result<Option<Disconnect>, Failure> {
    let mut sequence = activation.create();
    let mut buf = ironrdp_core::WriteBuf::new();
    loop {
        buf.clear();
        // Read here rather than in IronRDP's step, which turns the server's
        // Set Error Info into English.
        let written = match sequence.next_pdu_hint() {
            Some(hint) => {
                let pdu = reader
                    .read_by_hint(hint)
                    .await
                    .map_err(|e| Failure::Lost(describe(&e)))?;
                if let Some(reason) =
                    super::compat::error_info(&pdu).and_then(Disconnect::from_error_info)
                {
                    return Ok(Some(reason));
                }
                sequence.step(&pdu, reader.last_read_at(), &mut buf)
            }
            None => sequence.step_no_input(&mut buf),
        }
        .map_err(|e| failed(&e, desktop.shown))?;
        if written.size().is_some() {
            writer
                .write_all(buf.filled())
                .await
                .map_err(|e| Failure::Lost(describe(&e)))?;
        }
        if let ConnectionActivationState::Finalized {
            desktop_size,
            share_id,
            static_channel_chunk_size,
            enable_server_pointer,
            pointer_software_rendering,
            ..
        } = sequence.connection_activation_state()
        {
            if !stage.reactivate(
                sequence.io_channel_id(),
                sequence.user_channel_id(),
                share_id,
                enable_server_pointer,
                pointer_software_rendering,
                static_channel_chunk_size,
            ) {
                return Err(Failure::Protocol(
                    "the server's channel settings are invalid".into(),
                ));
            }
            *desktop = Desktop::new(desktop_size.width, desktop_size.height);
            return Ok(None);
        }
    }
}

/// A session error. A hang-up before the first frame, with no reason given,
/// is KRdp's answer to a wrong password.
fn failed(error: &(dyn std::error::Error + 'static), shown: bool) -> Failure {
    let text = describe(error);
    if !shown && text.contains("disconnect provider ultimatum") {
        Failure::Rejected
    } else {
        Failure::Protocol(text)
    }
}

fn rect(region: &InclusiveRectangle) -> Rect {
    Rect {
        x: i32::from(region.left),
        y: i32::from(region.top),
        width: i32::from(region.width()),
        height: i32::from(region.height()),
    }
}

async fn send(events: &mpsc::Sender<Event>, event: Event) -> Result<(), Failure> {
    // The interface has gone: nobody is left to show the session to.
    events
        .send(event)
        .await
        .map_err(|_| Failure::Lost("the window closed".into()))
}
