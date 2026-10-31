//! H.264 for the Graphics Pipeline, from whichever decoder the system has,
//! loaded when a session starts: FFmpeg's (the Flatpak's GNOME runtime has
//! it), the system's OpenH264 (Fedora's), or Cisco's OpenH264 downloaded on
//! request. Without one, servers send RemoteFX instead, or, like KRdp,
//! refuse the session.

pub(crate) mod cisco;
mod ffmpeg;
mod openh264;

use ironrdp_egfx::decode::{DecodedFrame, DecoderResult, H264Decoder, Yuv420Frame};

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

/// The decoder the graphics pipeline gets: timed for `mote::stats`, and,
/// for AVC420 only, saying it can't give planes, which is what keeps
/// IronRDP from offering AVC444.
pub(super) struct Decoder {
    inner: Box<dyn H264Decoder>,
    planes: bool,
    stats: crate::stats::Stats,
}

impl Decoder {
    pub(super) fn new(inner: Box<dyn H264Decoder>, avc444: bool) -> Self {
        Self {
            inner,
            planes: avc444,
            stats: crate::stats::Stats::new("h264"),
        }
    }
}

impl H264Decoder for Decoder {
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
        let start = std::time::Instant::now();
        let frame = self.inner.decode(data);
        self.stats.time("decode", start);
        frame
    }

    fn decode_yuv420(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
        let start = std::time::Instant::now();
        let frame = self.inner.decode_yuv420(data);
        self.stats.time("decode", start);
        frame
    }

    fn supports_yuv420(&self) -> bool {
        self.planes && self.inner.supports_yuv420()
    }

    fn wait_until_ready(&mut self) -> bool {
        self.inner.wait_until_ready()
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}
