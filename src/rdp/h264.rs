//! H.264 for the Graphics Pipeline, from whichever decoder the system has,
//! loaded when a session starts: FFmpeg's (the Flatpak's GNOME runtime has
//! it), the system's OpenH264 (Fedora's), or Cisco's OpenH264 downloaded on
//! request. Without one, servers send RemoteFX instead, or, like KRdp,
//! refuse the session.

pub(crate) mod cisco;
mod ffmpeg;
mod openh264;

use ironrdp_egfx::decode::H264Decoder;

/// Where H.264 comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Ffmpeg,
    SystemOpenH264,
    CiscoOpenH264,
}

/// A decoder from the first source that has one. `MOTE_H264` set to
/// `ffmpeg`, `openh264` or `cisco` tries that source alone, and
/// `MOTE_FFMPEG_DIR` names a directory to load FFmpeg from.
pub(super) fn decoder() -> Option<(Source, Box<dyn H264Decoder>)> {
    let only = std::env::var("MOTE_H264").ok();
    let wanted = |name: &str| only.as_deref().is_none_or(|o| o == name);
    let boxed = |d: Box<dyn H264Decoder>, s: Source| Some((s, d));
    if wanted("ffmpeg") {
        let dir = std::env::var_os("MOTE_FFMPEG_DIR").map(std::path::PathBuf::from);
        let found = dir
            .as_deref()
            .and_then(|d| ffmpeg::Ffmpeg::load(Some(d)))
            .or_else(|| ffmpeg::Ffmpeg::load(None));
        if let Some(d) = found {
            return boxed(Box::new(d), Source::Ffmpeg);
        }
    }
    if wanted("openh264")
        && let Some(d) = openh264::OpenH264::system()
    {
        return boxed(Box::new(d), Source::SystemOpenH264);
    }
    if wanted("cisco")
        && cisco::downloaded()
        && let Some(d) = openh264::OpenH264::checked(&cisco::path())
    {
        return boxed(Box::new(d), Source::CiscoOpenH264);
    }
    tracing::info!("no H.264 decoder: servers will send RemoteFX instead");
    None
}

/// Where H.264 would come from now, for Settings to show. Loads and drops a
/// decoder, so call it off the main thread.
pub(crate) fn source() -> Option<Source> {
    decoder().map(|(source, _)| source)
}
