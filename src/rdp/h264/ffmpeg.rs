//! H.264 through FFmpeg's libavcodec, loaded at run time: the Flatpak's
//! GNOME runtime carries it in its codecs-extra extension, and so do most
//! distributions. Only FFmpeg's stable functions are called; the one
//! structure read directly, the decoded frame, keeps the fields read here
//! at the same places in FFmpeg 5 to 8.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;

use ironrdp_egfx::decode::{DecodedFrame, DecoderError, DecoderResult, H264Decoder, Yuv420Frame};
use libloading::Library;

/// libavcodec and the libavutil that goes with it, newest first.
const VERSIONS: &[(&str, &str)] = &[
    ("libavcodec.so.62", "libavutil.so.60"),
    ("libavcodec.so.61", "libavutil.so.59"),
    ("libavcodec.so.60", "libavutil.so.58"),
    ("libavcodec.so.59", "libavutil.so.57"),
];

const AV_CODEC_ID_H264: c_int = 27;
const AV_PIX_FMT_YUV420P: c_int = 0;
const AV_PIX_FMT_YUVJ420P: c_int = 12;
const AV_INPUT_BUFFER_PADDING_SIZE: usize = 64;
const AV_LOG_QUIET: c_int = -8;
const AVERROR_EAGAIN: c_int = -11;

/// The start of `AVFrame`, unchanged since FFmpeg 5.
#[repr(C)]
struct AvFrameHead {
    data: [*mut u8; 8],
    linesize: [c_int; 8],
    extended_data: *mut *mut u8,
    width: c_int,
    height: c_int,
    nb_samples: c_int,
    format: c_int,
}

struct Api {
    find_decoder: unsafe extern "C" fn(c_int) -> *const c_void,
    alloc_context: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    open: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> c_int,
    free_context: unsafe extern "C" fn(*mut *mut c_void),
    flush: unsafe extern "C" fn(*mut c_void),
    send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    packet_free: unsafe extern "C" fn(*mut *mut c_void),
    packet_from_data: unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int,
    packet_unref: unsafe extern "C" fn(*mut c_void),
    frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    frame_free: unsafe extern "C" fn(*mut *mut c_void),
    frame_unref: unsafe extern "C" fn(*mut c_void),
    malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    opt_set: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int) -> c_int,
    log_set_level: unsafe extern "C" fn(c_int),
    // Kept loaded for as long as the functions above are used.
    _codec: Library,
    _util: Library,
}

impl Api {
    /// SAFETY: the libraries must be FFmpeg's, of a matching pair.
    unsafe fn load(codec: &str, util: &str) -> Result<Self, libloading::Error> {
        unsafe {
            let codec = Library::new(codec)?;
            let util = Library::new(util)?;
            Ok(Self {
                find_decoder: *codec.get(b"avcodec_find_decoder\0")?,
                alloc_context: *codec.get(b"avcodec_alloc_context3\0")?,
                open: *codec.get(b"avcodec_open2\0")?,
                free_context: *codec.get(b"avcodec_free_context\0")?,
                flush: *codec.get(b"avcodec_flush_buffers\0")?,
                send_packet: *codec.get(b"avcodec_send_packet\0")?,
                receive_frame: *codec.get(b"avcodec_receive_frame\0")?,
                packet_alloc: *codec.get(b"av_packet_alloc\0")?,
                packet_free: *codec.get(b"av_packet_free\0")?,
                packet_from_data: *codec.get(b"av_packet_from_data\0")?,
                packet_unref: *codec.get(b"av_packet_unref\0")?,
                frame_alloc: *util.get(b"av_frame_alloc\0")?,
                frame_free: *util.get(b"av_frame_free\0")?,
                frame_unref: *util.get(b"av_frame_unref\0")?,
                malloc: *util.get(b"av_malloc\0")?,
                opt_set: *util.get(b"av_opt_set\0")?,
                log_set_level: *util.get(b"av_log_set_level\0")?,
                _codec: codec,
                _util: util,
            })
        }
    }
}

pub(super) struct Ffmpeg {
    api: Api,
    context: *mut c_void,
    packet: *mut c_void,
    frame: *mut c_void,
    annex_b: Vec<u8>,
}

// SAFETY: the decoder is used from one thread at a time, as FFmpeg allows.
unsafe impl Send for Ffmpeg {}

impl Ffmpeg {
    /// A decoder from the first FFmpeg found with an H.264 decoder that
    /// opens; `dir` is looked in first.
    pub(super) fn load(dir: Option<&Path>) -> Option<Self> {
        for (codec, util) in VERSIONS {
            let (codec, util) = match dir {
                Some(dir) => (dir.join(codec), dir.join(util)),
                None => ((*codec).into(), (*util).into()),
            };
            // SAFETY: the names are FFmpeg's, paired by version.
            let api = match unsafe { Api::load(&codec.to_string_lossy(), &util.to_string_lossy()) }
            {
                Ok(api) => api,
                Err(e) => {
                    tracing::debug!("FFmpeg at {}: {e}", codec.display());
                    continue;
                }
            };
            match Self::open(api) {
                Ok(decoder) => {
                    tracing::debug!("H.264 through {}", codec.display());
                    return Some(decoder);
                }
                Err(e) => tracing::debug!("FFmpeg at {}: {e}", codec.display()),
            }
        }
        None
    }

    fn open(api: Api) -> Result<Self, String> {
        // SAFETY: each call follows FFmpeg's documented use; what is
        // allocated here is freed in `Drop`.
        unsafe {
            (api.log_set_level)(AV_LOG_QUIET);
            let codec = (api.find_decoder)(AV_CODEC_ID_H264);
            if codec.is_null() {
                return Err("no H.264 decoder".into());
            }
            // Fedora's FFmpeg has no H.264 decoder of its own, only
            // OpenH264's, wrapped so that it holds each picture back to put
            // frames in order: a still desktop then never shows, and
            // AVC444's two streams come apart. The same OpenH264 called
            // directly gives each picture as it comes. `name` is the first
            // field of AVCodec in every FFmpeg.
            let name = CStr::from_ptr(*codec.cast::<*const c_char>());
            if name.to_bytes() == b"libopenh264" {
                return Err("its H.264 decoder is OpenH264's, used directly instead".into());
            }
            let context = (api.alloc_context)(codec);
            if context.is_null() {
                return Err("no decoder context".into());
            }
            // Low delay: each frame out as soon as it is in. Threads split
            // a frame, never hold one back.
            for (name, value) in [
                ("flags", "+low_delay"),
                ("thread_type", "slice"),
                ("threads", "0"),
            ] {
                let (name, value) = (
                    CString::new(name).unwrap_or_default(),
                    CString::new(value).unwrap_or_default(),
                );
                (api.opt_set)(context, name.as_ptr(), value.as_ptr(), 0);
            }
            let packet = (api.packet_alloc)();
            let frame = (api.frame_alloc)();
            let mut decoder = Self {
                api,
                context,
                packet,
                frame,
                annex_b: Vec::new(),
            };
            if packet.is_null() || frame.is_null() {
                return Err("out of memory".into());
            }
            if (decoder.api.open)(context, codec, std::ptr::null_mut()) < 0 {
                decoder.context = std::ptr::null_mut();
                (decoder.api.free_context)(&mut { context });
                return Err("the H.264 decoder doesn't open".into());
            }
            Ok(decoder)
        }
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        // SAFETY: freeing what `open` allocated; FFmpeg's free functions
        // take a null pointer as nothing to do.
        unsafe {
            (self.api.frame_free)(&mut self.frame);
            (self.api.packet_free)(&mut self.packet);
            if !self.context.is_null() {
                (self.api.free_context)(&mut self.context);
            }
        }
    }
}

impl H264Decoder for Ffmpeg {
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
        Ok(self.decode_yuv420(data)?.to_rgba())
    }

    fn decode_yuv420(&mut self, data: &[u8]) -> DecoderResult<Yuv420Frame> {
        let stream = if ironrdp_egfx::pdu::is_avc_format(data) {
            ironrdp_egfx::pdu::avc_to_annex_b_into(data, &mut self.annex_b);
            &self.annex_b
        } else {
            data
        };
        let size =
            c_int::try_from(stream.len()).map_err(|_| DecoderError::msg("packet too large"))?;
        // SAFETY: the packet's buffer comes from av_malloc, with the padding
        // FFmpeg reads past the end, and passes to the packet, which frees
        // it on unref. The frame's planes are read within its own sizes.
        unsafe {
            let buffer =
                (self.api.malloc)(stream.len() + AV_INPUT_BUFFER_PADDING_SIZE).cast::<u8>();
            if buffer.is_null() {
                return Err(DecoderError::msg("out of memory"));
            }
            std::ptr::copy_nonoverlapping(stream.as_ptr(), buffer, stream.len());
            std::ptr::write_bytes(buffer.add(stream.len()), 0, AV_INPUT_BUFFER_PADDING_SIZE);
            if (self.api.packet_from_data)(self.packet, buffer, size) < 0 {
                return Err(DecoderError::msg("FFmpeg refused the packet"));
            }
            let sent = (self.api.send_packet)(self.context, self.packet);
            (self.api.packet_unref)(self.packet);
            if sent < 0 && sent != AVERROR_EAGAIN {
                return Err(DecoderError::msg(format!("FFmpeg decode failed ({sent})")));
            }
            let received = (self.api.receive_frame)(self.context, self.frame);
            if received < 0 {
                return Err(DecoderError::msg("FFmpeg returned no picture"));
            }
            let picture = self.copy_picture();
            (self.api.frame_unref)(self.frame);
            picture
        }
    }

    fn supports_yuv420(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        // SAFETY: the context is open.
        unsafe { (self.api.flush)(self.context) }
    }
}

impl Ffmpeg {
    /// The decoded frame's planes, copied out.
    ///
    /// SAFETY: `self.frame` holds a decoded picture.
    unsafe fn copy_picture(&self) -> DecoderResult<Yuv420Frame> {
        let head = unsafe { &*self.frame.cast::<AvFrameHead>() };
        if head.format != AV_PIX_FMT_YUV420P && head.format != AV_PIX_FMT_YUVJ420P {
            return Err(DecoderError::msg(format!(
                "unexpected pixel format {}",
                head.format
            )));
        }
        let (Ok(width), Ok(height)) = (u16::try_from(head.width), u16::try_from(head.height))
        else {
            return Err(DecoderError::msg("picture size out of range"));
        };
        let strides = [head.linesize[0], head.linesize[1], head.linesize[2]];
        if strides.iter().any(|s| *s <= 0) || strides[1] != strides[2] {
            return Err(DecoderError::msg("unexpected plane layout"));
        }
        let (luma, chroma) = (strides[0] as usize, strides[1] as usize);
        let chroma_rows = usize::from(height).div_ceil(2);
        let plane = |index: usize, len: usize| unsafe {
            std::slice::from_raw_parts(head.data[index], len).to_vec()
        };
        Yuv420Frame::new(
            width,
            height,
            plane(0, luma * usize::from(height)),
            luma,
            plane(1, chroma * chroma_rows),
            plane(2, chroma * chroma_rows),
            chroma,
        )
    }
}
