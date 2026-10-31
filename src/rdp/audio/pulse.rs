//! PulseAudio's simple API, loaded at run time. PipeWire serves it on
//! every desktop it runs, and a Flatpak reaches it through the usual
//! `--socket=pulseaudio`, where PipeWire's own socket is out of reach.
//! Each stream blocks, so it lives on a thread of its own.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;
use std::time::Duration;

use libloading::Library;

const PA_SAMPLE_S16LE: c_int = 3;
const PA_STREAM_PLAYBACK: c_int = 1;
const PA_STREAM_RECORD: c_int = 2;

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

/// What the server picks, left to it.
const DEFAULT: u32 = u32::MAX;

type New = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const SampleSpec,
    *const c_void,
    *const BufferAttr,
    *mut c_int,
) -> *mut c_void;

struct Api {
    new: New,
    write: unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int,
    read: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_int) -> c_int,
    latency: unsafe extern "C" fn(*mut c_void, *mut c_int) -> u64,
    flush: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    // Kept loaded for as long as the functions above are used.
    _library: Library,
}

impl Api {
    /// SAFETY: the library must be PulseAudio's simple API.
    unsafe fn load() -> Result<Self, libloading::Error> {
        unsafe {
            let library = Library::new("libpulse-simple.so.0")?;
            Ok(Self {
                new: *library.get(b"pa_simple_new\0")?,
                write: *library.get(b"pa_simple_write\0")?,
                read: *library.get(b"pa_simple_read\0")?,
                latency: *library.get(b"pa_simple_get_latency\0")?,
                flush: *library.get(b"pa_simple_flush\0")?,
                free: *library.get(b"pa_simple_free\0")?,
                // From libpulse, which the simple API brings along.
                strerror: *library.get(b"pa_strerror\0")?,
                _library: library,
            })
        }
    }

    fn error(&self, code: c_int) -> String {
        // SAFETY: pa_strerror returns a static string, or null.
        let text = unsafe { (self.strerror)(code) };
        if text.is_null() {
            return format!("error {code}");
        }
        // SAFETY: checked for null above; the string is static.
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: the library by that name is PulseAudio's.
        match unsafe { Api::load() } {
            Ok(api) => Some(api),
            Err(e) => {
                tracing::warn!("no sound: PulseAudio's library: {e}");
                None
            }
        }
    })
    .as_ref()
}

/// Whether sound can be played and recorded here at all.
pub(super) fn available() -> bool {
    api().is_some()
}

/// One stream of 16-bit samples, to the speakers or from the microphone.
pub(super) struct Stream {
    api: &'static Api,
    handle: *mut c_void,
}

// SAFETY: a stream is used by one thread at a time, which is all the
// simple API asks; it is made on one thread and kept there.
unsafe impl Send for Stream {}

impl Stream {
    /// A stream to the default speakers that keeps about `buffer` queued.
    pub(super) fn play(
        name: &str,
        rate: u32,
        channels: u8,
        buffer: Duration,
    ) -> Result<Self, String> {
        let bytes = bytes_for(buffer, rate, channels);
        let attr = BufferAttr {
            maxlength: DEFAULT,
            tlength: bytes,
            prebuf: DEFAULT,
            minreq: DEFAULT,
            fragsize: DEFAULT,
        };
        Self::open(PA_STREAM_PLAYBACK, name, rate, channels, &attr)
    }

    /// A stream from the default microphone, read `fragment` at a time.
    pub(super) fn record(
        name: &str,
        rate: u32,
        channels: u8,
        fragment: Duration,
    ) -> Result<Self, String> {
        let attr = BufferAttr {
            maxlength: DEFAULT,
            tlength: DEFAULT,
            prebuf: DEFAULT,
            minreq: DEFAULT,
            fragsize: bytes_for(fragment, rate, channels),
        };
        Self::open(PA_STREAM_RECORD, name, rate, channels, &attr)
    }

    fn open(
        direction: c_int,
        name: &str,
        rate: u32,
        channels: u8,
        attr: &BufferAttr,
    ) -> Result<Self, String> {
        let api = api().ok_or("PulseAudio's library is missing")?;
        let spec = SampleSpec {
            format: PA_SAMPLE_S16LE,
            rate,
            channels,
        };
        let app = CString::new(crate::APP_NAME).unwrap_or_default();
        let stream = CString::new(name).unwrap_or_default();
        let mut error = 0;
        // SAFETY: every pointer is valid for the call; null server, device
        // and channel map are the defaults.
        let handle = unsafe {
            (api.new)(
                std::ptr::null(),
                app.as_ptr(),
                direction,
                std::ptr::null(),
                stream.as_ptr(),
                &spec,
                std::ptr::null(),
                attr,
                &mut error,
            )
        };
        if handle.is_null() {
            return Err(api.error(error));
        }
        Ok(Self { api, handle })
    }

    /// Queue samples; blocks while the buffer is full.
    pub(super) fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut error = 0;
        // SAFETY: the handle is open and the data valid for its length.
        let done =
            unsafe { (self.api.write)(self.handle, data.as_ptr().cast(), data.len(), &mut error) };
        if done < 0 {
            return Err(self.api.error(error));
        }
        Ok(())
    }

    /// Fill `data` with recorded samples; blocks until there are enough.
    pub(super) fn read(&mut self, data: &mut [u8]) -> Result<(), String> {
        let mut error = 0;
        // SAFETY: the handle is open and the buffer valid for its length.
        let done = unsafe {
            (self.api.read)(
                self.handle,
                data.as_mut_ptr().cast(),
                data.len(),
                &mut error,
            )
        };
        if done < 0 {
            return Err(self.api.error(error));
        }
        Ok(())
    }

    /// How long until what is queued now is heard.
    pub(super) fn latency(&mut self) -> Duration {
        let mut error = 0;
        // SAFETY: the handle is open.
        let micros = unsafe { (self.api.latency)(self.handle, &mut error) };
        if micros == u64::MAX {
            return Duration::ZERO;
        }
        Duration::from_micros(micros)
    }

    /// Drop what is queued and not yet played.
    pub(super) fn flush(&mut self) {
        let mut error = 0;
        // SAFETY: the handle is open.
        unsafe { (self.api.flush)(self.handle, &mut error) };
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the handle is open, and not used again.
        unsafe { (self.api.free)(self.handle) };
    }
}

fn bytes_for(duration: Duration, rate: u32, channels: u8) -> u32 {
    let frames = u64::from(rate) * duration.as_millis() as u64 / 1000;
    (frames * u64::from(channels) * 2) as u32
}
