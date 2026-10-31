//! H.264 for the Graphics Pipeline, through Cisco's OpenH264 library as the
//! system provides it (Fedora's `openh264` package, Flathub's OpenH264
//! extension), loaded when a session starts. Cisco's builds carry its patent
//! license, which is why the library is loaded rather than built in.
//!
//! IronRDP's own OpenH264 decoder accepts only the builds Cisco publishes
//! itself, by hash; the distributions build theirs from the same sources,
//! so this one loads whatever the system has.

use ironrdp_egfx::decode::{DecodedFrame, DecoderError, DecoderResult, H264Decoder, Yuv420Frame};
use openh264::OpenH264API;
use openh264::formats::YUVSource;

/// The library names to try: OpenH264 2.6's ABI, which the bindings are for.
const LIBRARIES: &[&str] = &["libopenh264.so.8"];

pub(super) struct OpenH264 {
    decoder: openh264::decoder::Decoder,
    annex_b: Vec<u8>,
}

impl OpenH264 {
    /// A decoder, or `None` when the system has no OpenH264: the server then
    /// sends RemoteFX instead. `MOTE_OPENH264` names a library to use.
    pub(super) fn load() -> Option<Self> {
        let names: Vec<String> = std::env::var("MOTE_OPENH264")
            .into_iter()
            .chain(LIBRARIES.iter().map(|s| (*s).to_owned()))
            .collect();
        for name in &names {
            // SAFETY: the library is the system's OpenH264, loaded for its
            // C API only; the bindings check its version on creation.
            let api = match unsafe { OpenH264API::from_blob_path_unchecked(name) } {
                Ok(api) => api,
                Err(e) => {
                    tracing::debug!("OpenH264 at {name}: {e}");
                    continue;
                }
            };
            match openh264::decoder::Decoder::with_api_config(api, Default::default()) {
                Ok(decoder) => {
                    tracing::debug!("H.264 through {name}");
                    return Some(Self {
                        decoder,
                        annex_b: Vec::new(),
                    });
                }
                Err(e) => tracing::warn!("OpenH264 at {name}: {e}"),
            }
        }
        tracing::info!("no OpenH264: servers will send RemoteFX instead of H.264");
        None
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
