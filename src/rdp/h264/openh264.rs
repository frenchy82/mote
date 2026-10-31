//! H.264 through Cisco's OpenH264 library: the system's (Fedora's
//! `openh264` package), or Cisco's own build downloaded on request.
//!
//! IronRDP's OpenH264 decoder accepts only the builds Cisco publishes, by
//! hash. The distributions build theirs from the same sources, so the
//! system's is loaded without that check; the download keeps it.

use std::path::Path;

use ironrdp_egfx::decode::{DecodedFrame, DecoderError, DecoderResult, H264Decoder, Yuv420Frame};
use openh264::OpenH264API;
use openh264::formats::YUVSource;

/// The system library names to try: OpenH264 2.6's ABI, which the bindings
/// are for.
const LIBRARIES: &[&str] = &["libopenh264.so.8"];

pub(super) struct OpenH264 {
    decoder: openh264::decoder::Decoder,
    annex_b: Vec<u8>,
}

impl OpenH264 {
    /// The system's OpenH264, or the library `MOTE_OPENH264` names.
    pub(super) fn system() -> Option<Self> {
        let names: Vec<String> = std::env::var("MOTE_OPENH264")
            .into_iter()
            .chain(LIBRARIES.iter().map(|s| (*s).to_owned()))
            .collect();
        names.iter().find_map(|name| {
            // SAFETY: the library is the system's OpenH264, loaded for its
            // C API only; the bindings check its version on creation.
            match unsafe { OpenH264API::from_blob_path_unchecked(name) } {
                Ok(api) => Self::open(api, name),
                Err(e) => {
                    tracing::debug!("OpenH264 at {name}: {e}");
                    None
                }
            }
        })
    }

    /// Cisco's build at `path`, loaded only if it is one Cisco published.
    pub(super) fn checked(path: &Path) -> Option<Self> {
        match OpenH264API::from_blob_path(path) {
            Ok(api) => Self::open(api, &path.display().to_string()),
            Err(e) => {
                tracing::warn!("OpenH264 at {}: {e}", path.display());
                None
            }
        }
    }

    /// A decoder, unless the library is a stand-in that can't make one, as
    /// Fedora's default `noopenh264` is.
    fn open(api: OpenH264API, name: &str) -> Option<Self> {
        match openh264::decoder::Decoder::with_api_config(api, Default::default()) {
            Ok(decoder) => {
                tracing::debug!("H.264 through {name}");
                Some(Self {
                    decoder,
                    annex_b: Vec::new(),
                })
            }
            Err(e) => {
                tracing::debug!("OpenH264 at {name}: {e}");
                None
            }
        }
    }
}

impl H264Decoder for OpenH264 {
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
        Ok(self.decode_yuv420(data)?.to_rgba())
    }

    fn decode_yuv420(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
        // The Graphics Pipeline's AVC420 streams may come as length-prefixed
        // NAL units; OpenH264 takes Annex B.
        let stream = if ironrdp_egfx::pdu::is_avc_format(data) {
            ironrdp_egfx::pdu::avc_to_annex_b_into(data, &mut self.annex_b);
            &self.annex_b
        } else {
            data
        };
        let picture = self
            .decoder
            .decode(stream)
            .map_err(|e| DecoderError::new("OpenH264 decode failed", e))?
            .ok_or_else(|| DecoderError::msg("OpenH264 returned no picture"))?;
        let (width, height) = picture.dimensions();
        let (y_stride, u_stride, v_stride) = picture.strides();
        let (Ok(width), Ok(height)) = (u16::try_from(width), u16::try_from(height)) else {
            return Err(DecoderError::msg("picture larger than 65535 pixels"));
        };
        if u_stride != v_stride {
            return Err(DecoderError::msg("chroma planes with different strides"));
        }
        Yuv420Frame::new(
            width,
            height,
            picture.y().to_vec(),
            y_stride,
            picture.u().to_vec(),
            picture.v().to_vec(),
            u_stride,
        )
    }

    fn supports_yuv420(&self) -> bool {
        true
    }
}
