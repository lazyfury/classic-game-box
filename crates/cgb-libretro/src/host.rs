//! The libretro **front end**: it owns the callbacks the core calls back into,
//! and the small amount of state they need.
//!
//! ## Why a global pointer
//!
//! libretro callbacks (`video_refresh`, `input_state`, …) carry no user context
//! pointer, so a single-instance host has to stash one somewhere reachable from
//! an `extern "C"` function. [`HOST`] is that stash: set when a [`CoreHost`] is
//! created, cleared when it drops. One core is live at a time — [`CoreHost::new`]
//! refuses a second host while one is alive, and the app drops the old session
//! before starting a new one — so this is not a race, but it *is* a process-wide
//! singleton and is documented as such.
//!
//! ## Callback ordering
//!
//! libretro requires `retro_set_environment` before `retro_init`, and the
//! video/audio/input callbacks before `retro_load_game`. [`CoreHost::new`]
//! and [`CoreHost::load_game`] enforce that order.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicI16, AtomicPtr, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cgb_systems::{
    RETRO_DEVICE_ANALOG, RETRO_DEVICE_ANALOG_BIT, RETRO_DEVICE_ID_ANALOG_X,
    RETRO_DEVICE_ID_ANALOG_Y, RETRO_DEVICE_ID_JOYPAD_MASK, RETRO_DEVICE_INDEX_ANALOG_LEFT,
    RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_JOYPAD, RETRO_DEVICE_JOYPAD_BIT,
};

use crate::error::LibretroError;
use crate::ffi::*;
use crate::loader::CoreLibrary;

/// The one live host, reachable from the `extern "C"` callbacks.
///
/// `null` when no core is loaded. Single-instance by design; see the module
/// docs.
static HOST: AtomicPtr<HostShared> = AtomicPtr::new(ptr::null_mut());

/// A frame the core produced, already converted to straight RGBA8.
///
/// libretro hands over XRGB8888 (little-endian `B G R X`); quill's wgpu backend
/// samples RGBA8, so the host does the swizzle on the way in. See
/// `docs/architecture/quill-native-migration.md` §5.3.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, RGBA order.
    pub rgba: Vec<u8>,
}

/// A core's `retro_get_system_info` result, owned.
#[derive(Clone, Debug, Default)]
pub struct SystemInfo {
    pub library_name: String,
    pub library_version: String,
    pub valid_extensions: Vec<String>,
    pub need_fullpath: bool,
}

/// A core's `retro_get_system_av_info` result, owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct AvInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub sample_rate: f64,
}

/// One `retro_input_descriptor`, owned. Drives the bindings UI.
#[derive(Clone, Debug)]
pub struct InputDescriptor {
    pub port: u32,
    pub device: u32,
    pub index: u32,
    pub id: u32,
    pub description: String,
}

/// State shared with the callbacks.
struct HostShared {
    /// From `RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY`; stable for the host's
    /// life so the raw pointer handed to the core stays valid.
    system_dir: CString,
    /// From `RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY`.
    save_dir: CString,
    /// The format the core asked for (`RETRO_PIXEL_FORMAT_*`). Defaults to
    /// 0RGB1555, which is what libretro assumes before `SET_PIXEL_FORMAT`.
    pixel_format: AtomicU32,
    /// The most recent frame, taken by the app with [`CoreHost::take_frame`].
    video: Mutex<Option<Frame>>,
    /// Interleaved int16 stereo samples, drained by [`CoreHost::take_audio`].
    audio: Mutex<Vec<i16>>,
    /// Button bitmasks, one per port.
    input: [AtomicU16; 2],
    /// Analog axes, one per port, indexed `stick * 2 + axis` (`stick` 0 left /
    /// 1 right; `axis` 0 X / 1 Y), in libretro's i16 range.
    analog: [[AtomicI16; 4]; 2],
    /// Descriptors from `SET_INPUT_DESCRIPTORS`, for the bindings UI.
    input_descriptors: Mutex<Vec<InputDescriptor>>,
}

impl HostShared {
    fn new(system_dir: &Path, save_dir: &Path) -> Self {
        let c = |path: &Path| CString::new(path.to_string_lossy().into_owned()).unwrap_or_default();
        Self {
            system_dir: c(system_dir),
            save_dir: c(save_dir),
            pixel_format: AtomicU32::new(RETRO_PIXEL_FORMAT_0RGB1555),
            video: Mutex::new(None),
            audio: Mutex::new(Vec::new()),
            input: [AtomicU16::new(0), AtomicU16::new(0)],
            analog: Default::default(),
            input_descriptors: Mutex::new(Vec::new()),
        }
    }

    /// Copy one frame and swizzle it to RGBA8.
    ///
    /// # Safety
    ///
    /// `data` points to `pitch * height` readable bytes in the core's frame
    /// format, as libretro promises for the duration of the callback.
    unsafe fn on_video(&self, data: *const c_void, width: u32, height: u32, pitch: usize) {
        if data.is_null() || width == 0 || height == 0 {
            // `GET_CAN_DUPE` is true: no data means "repeat the last frame".
            return;
        }
        let format = self.pixel_format.load(Ordering::Relaxed);
        let w = width as usize;
        let h = height as usize;
        // Bytes per source pixel: 32-bit XRGB8888, 16-bit RGB565/0RGB1555.
        // The row slice must use this, not a hardcoded 4 — mGBA hands over
        // RGB565 at pitch 512 for a 240-wide frame, where `w * 4` overruns.
        let bpp = match format {
            RETRO_PIXEL_FORMAT_XRGB8888 => 4,
            _ => 2,
        };
        // A row must hold the pixels we are about to read. Refuse rather than
        // panic: this runs inside an `extern "C"` callback, where a panic
        // aborts the process.
        if pitch < w * bpp {
            return;
        }
        let bytes = std::slice::from_raw_parts(data as *const u8, pitch * h);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            let row = &bytes[y * pitch..y * pitch + w * bpp];
            for x in 0..w {
                let px = &mut rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                match format {
                    RETRO_PIXEL_FORMAT_XRGB8888 => {
                        px[0] = row[x * 4 + 2]; // R
                        px[1] = row[x * 4 + 1]; // G
                        px[2] = row[x * 4]; // B
                        px[3] = 255;
                    }
                    RETRO_PIXEL_FORMAT_RGB565 => {
                        let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                        px[0] = scale5(((v >> 11) & 0x1f) as u8);
                        px[1] = scale6(((v >> 5) & 0x3f) as u8);
                        px[2] = scale5((v & 0x1f) as u8);
                        px[3] = 255;
                    }
                    // 0RGB1555, libretro's default before SET_PIXEL_FORMAT.
                    _ => {
                        let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                        px[0] = scale5(((v >> 10) & 0x1f) as u8);
                        px[1] = scale5(((v >> 5) & 0x1f) as u8);
                        px[2] = scale5((v & 0x1f) as u8);
                        px[3] = 255;
                    }
                }
            }
        }
        *lock(&self.video) = Some(Frame {
            width,
            height,
            rgba,
        });
    }

    fn on_audio_batch(&self, samples: &[i16]) {
        let mut queue = lock(&self.audio);
        queue.extend_from_slice(samples);
        // Bound the queue so a stalled output cannot grow without limit.
        const MAX: usize = 192_000; // ~2s stereo @ 48kHz
        if queue.len() > MAX {
            let excess = queue.len() - MAX;
            queue.drain(0..excess);
        }
    }

    /// The live libretro environment handler. Returns whether the command is
    /// supported, exactly as `retro_environment_t` promises.
    ///
    /// # Safety
    ///
    /// `data` is interpreted according to `cmd`; the pointer types are fixed by
    /// `libretro.h`.
    unsafe fn environment(&self, cmd: c_uint, data: *mut c_void) -> bool {
        match cmd {
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
                if data.is_null() {
                    return false;
                }
                let format = *(data as *const c_uint);
                // Accept what `on_video` can convert. Only record the format
                // when accepted: returning false tells the core to keep
                // 0RGB1555, and recording RGB565 anyway would then read its
                // output with the wrong conversion.
                match format {
                    RETRO_PIXEL_FORMAT_XRGB8888 | RETRO_PIXEL_FORMAT_RGB565 => {
                        self.pixel_format.store(format, Ordering::Relaxed);
                        true
                    }
                    _ => false,
                }
            }
            RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
                if data.is_null() {
                    return false;
                }
                *(data as *mut *const c_char) = self.system_dir.as_ptr();
                true
            }
            RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => {
                if data.is_null() {
                    return false;
                }
                *(data as *mut *const c_char) = self.save_dir.as_ptr();
                true
            }
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE => {
                if data.is_null() {
                    return false;
                }
                // MAME-family cores call this pointer unconditionally, so a
                // false here leaves them with a null function pointer and they
                // crash on the first log line. Always hand back a real sink.
                let callback = data as *mut RetroLogCallback;
                (*callback).log = core_log as *const () as *mut c_void;
                true
            }
            RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
                *lock(&self.input_descriptors) = read_input_descriptors(data);
                true
            }
            // We can answer both the button bitmask and analog axes, so a core
            // that reads a stick gets real values instead of nothing. The
            // capabilities value is a `uint64_t` of `1 << RETRO_DEVICE_*`.
            RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
                if !data.is_null() {
                    let caps = u64::from(RETRO_DEVICE_JOYPAD_BIT | RETRO_DEVICE_ANALOG_BIT);
                    *(data as *mut u64) = caps;
                }
                true
            }
            RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => {
                if !data.is_null() {
                    *(data as *mut bool) = true;
                }
                true
            }
            RETRO_ENVIRONMENT_SET_CONTROLLER_INFO => true,
            RETRO_ENVIRONMENT_GET_VARIABLE => {
                // No core options yet: an unset value tells the core to use its
                // own default.
                if !data.is_null() {
                    (*(data as *mut retro_variable)).value = ptr::null();
                }
                false
            }
            RETRO_ENVIRONMENT_SET_VARIABLES => true,
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                if !data.is_null() {
                    *(data as *mut bool) = false;
                }
                true
            }
            RETRO_ENVIRONMENT_GET_CAN_DUPE => {
                if !data.is_null() {
                    *(data as *mut bool) = true;
                }
                true
            }
            RETRO_ENVIRONMENT_GET_OVERSCAN => {
                if !data.is_null() {
                    *(data as *mut bool) = false;
                }
                true
            }
            // Accepted and ignored for now.
            RETRO_ENVIRONMENT_SET_ROTATION
            | RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
            | RETRO_ENVIRONMENT_SET_GEOMETRY
            | RETRO_ENVIRONMENT_SET_MESSAGE => true,
            // Everything else (core options callbacks, VFS, hw render, …) is
            // reported unsupported so the core degrades predictably.
            _ => false,
        }
    }
}

/// A loaded core, ready to load a game and run.
pub struct CoreHost {
    core: CoreLibrary,
    shared: Arc<HostShared>,
    loaded: bool,
    game_path: Option<PathBuf>,
}

impl CoreHost {
    /// Open `core_path` and bring it up: register the front end callbacks and
    /// call `retro_init`.
    pub fn new(
        core_path: impl AsRef<Path>,
        system_dir: impl AsRef<Path>,
        save_dir: impl AsRef<Path>,
    ) -> Result<Self, LibretroError> {
        // The callbacks reach exactly one host through `HOST`, and a core is a
        // single loaded instance. A second live host would `retro_init` the
        // same dylib again and then `retro_deinit` the new machine when the old
        // host drops — a segfault. Refuse up front instead.
        if !HOST.load(Ordering::Acquire).is_null() {
            return Err(LibretroError::HostBusy);
        }
        let core = CoreLibrary::open(core_path)?;
        let shared = Arc::new(HostShared::new(system_dir.as_ref(), save_dir.as_ref()));

        // Publish the context *before* `retro_init`: some cores call
        // `environment` from init.
        HOST.store(Arc::as_ptr(&shared) as *mut HostShared, Ordering::Release);

        unsafe {
            let api = core.api();
            (api.set_environment)(environment_cb);
            (api.init)();
            (api.set_video_refresh)(video_cb);
            (api.set_audio_sample)(audio_cb);
            (api.set_audio_sample_batch)(audio_batch_cb);
            (api.set_input_poll)(input_poll_cb);
            (api.set_input_state)(input_state_cb);
        }

        Ok(Self {
            core,
            shared,
            loaded: false,
            game_path: None,
        })
    }

    /// Load a cartridge from bytes. `path` is for the core's own format
    /// detection; the bytes are what it reads.
    pub fn load_game(&mut self, path: impl AsRef<Path>, data: &[u8]) -> Result<(), LibretroError> {
        if data.is_empty() {
            return Err(LibretroError::EmptyGame);
        }
        let path = path.as_ref();
        // Kept alive for the duration of the call; the core copies what it
        // needs.
        let cpath = CString::new(path.to_string_lossy().into_owned()).unwrap_or_default();
        let info = retro_game_info {
            path: cpath.as_ptr(),
            data: data.as_ptr() as *const c_void,
            size: data.len(),
            meta: ptr::null(),
        };
        let loaded = unsafe { (self.core.api().load_game)(&info) };
        if !loaded {
            return Err(LibretroError::LoadGame {
                path: path.display().to_string(),
            });
        }
        self.loaded = true;
        self.game_path = Some(path.to_path_buf());
        Ok(())
    }

    /// Run exactly one emulated frame. The core calls back into the front end
    /// for video, audio and input.
    pub fn run_frame(&self) {
        unsafe { (self.core.api().run)() }
    }

    /// Reset the machine without reloading it.
    pub fn reset(&self) {
        unsafe { (self.core.api().reset)() }
    }

    /// Take the frame produced since the last call, if any.
    pub fn take_frame(&self) -> Option<Frame> {
        lock(&self.shared.video).take()
    }

    /// Drain the audio produced since the last call (int16 stereo, interleaved).
    pub fn take_audio(&self) -> Vec<i16> {
        std::mem::take(&mut *lock(&self.shared.audio))
    }

    /// Set the pressed-button bitmask for a port (`0` or `1`).
    pub fn set_buttons(&self, port: usize, mask: u16) {
        if let Some(slot) = self.shared.input.get(port) {
            slot.store(mask, Ordering::Relaxed);
        }
    }

    /// Set one analog axis: `stick` 0 = left, 1 = right; `axis` 0 = X, 1 = Y.
    pub fn set_analog(&self, port: usize, stick: usize, axis: usize, value: i16) {
        if let Some(slot) = self
            .shared
            .analog
            .get(port)
            .and_then(|port| port.get(stick * 2 + axis))
        {
            slot.store(value, Ordering::Relaxed);
        }
    }

    /// The bindings the core describes, for the settings UI.
    pub fn input_descriptors(&self) -> Vec<InputDescriptor> {
        lock(&self.shared.input_descriptors).clone()
    }

    /// The core's system info (name, version, extensions).
    pub fn system_info(&self) -> SystemInfo {
        let mut info = retro_system_info {
            library_name: ptr::null(),
            library_version: ptr::null(),
            valid_extensions: ptr::null(),
            need_fullpath: false,
            block_extract: false,
        };
        unsafe { (self.core.api().system_info)(&mut info) };
        SystemInfo {
            library_name: cstring(info.library_name),
            library_version: cstring(info.library_version),
            valid_extensions: cstring(info.valid_extensions)
                .split('|')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            need_fullpath: info.need_fullpath,
        }
    }

    /// The core's current geometry and timing. Call it **after** `load_game`:
    /// mGBA only answers correctly once a machine is in.
    pub fn av_info(&self) -> AvInfo {
        let mut av = retro_system_av_info::default();
        unsafe { (self.core.api().av_info)(&mut av) };
        AvInfo {
            width: av.geometry.base_width,
            height: av.geometry.base_height,
            fps: av.timing.fps,
            sample_rate: av.timing.sample_rate,
        }
    }

    /// Instant save-state bytes, or empty if the core cannot serialize.
    pub fn serialize(&self) -> Vec<u8> {
        let size = unsafe { (self.core.api().serialize_size)() };
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size];
        let ok = unsafe { (self.core.api().serialize)(buf.as_mut_ptr() as *mut c_void, size) };
        if ok {
            buf
        } else {
            Vec::new()
        }
    }

    /// The number of bytes `serialize` would produce, or `0` when the core does
    /// not implement save states.
    pub fn serialize_size(&self) -> usize {
        unsafe { (self.core.api().serialize_size)() }
    }

    /// Restore a save state. Returns whether the core accepted it.
    pub fn unserialize(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        unsafe { (self.core.api().unserialize)(bytes.as_ptr() as *const c_void, bytes.len()) }
    }

    /// The battery save (`RETRO_MEMORY_SAVE_RAM`), copied out for persisting to
    /// `<save_dir>/<rom>.srm`.
    pub fn save_ram(&self) -> Option<Vec<u8>> {
        let size = unsafe { (self.core.api().get_memory_size)(RETRO_MEMORY_SAVE_RAM) };
        if size == 0 {
            return None;
        }
        let ptr = unsafe { (self.core.api().get_memory_data)(RETRO_MEMORY_SAVE_RAM) } as *const u8;
        if ptr.is_null() {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec())
    }

    /// Write battery-save bytes back into the core (called after loading a
    /// `.srm` from disk).
    pub fn write_save_ram(&self, bytes: &[u8]) -> bool {
        let size = unsafe { (self.core.api().get_memory_size)(RETRO_MEMORY_SAVE_RAM) };
        if size == 0 {
            return false;
        }
        let ptr = unsafe { (self.core.api().get_memory_data)(RETRO_MEMORY_SAVE_RAM) } as *mut u8;
        if ptr.is_null() {
            return false;
        }
        let n = size.min(bytes.len());
        unsafe { std::slice::from_raw_parts_mut(ptr, n).copy_from_slice(&bytes[..n]) };
        true
    }

    /// The cartridge currently loaded, if any.
    pub fn game_path(&self) -> Option<&Path> {
        self.game_path.as_deref()
    }
}

impl Drop for CoreHost {
    fn drop(&mut self) {
        // Unpublish first so no callback can reach a half-dropped host.
        HOST.store(ptr::null_mut(), Ordering::Release);
        unsafe {
            if self.loaded {
                (self.core.api().unload_game)();
            }
            (self.core.api().deinit)();
        }
    }
}

// --- callbacks ------------------------------------------------------------
//
// Each one fetches the global host, or does nothing when no core is loaded.
// They must not panic: unwinding across the FFI boundary is undefined
// behaviour, so the shared-state locks recover from poisoning instead.

/// The shared host, or `None` when no core is loaded.
fn current() -> Option<&'static HostShared> {
    let ptr = HOST.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the pointer is the `Arc::as_ptr` of a live `CoreHost.shared`,
        // cleared before that Arc is dropped. Reads only.
        Some(unsafe { &*ptr })
    }
}

/// `retro_log_callback`: the one function pointer a core logs through.
#[repr(C)]
struct RetroLogCallback {
    log: *mut c_void,
}

/// The front end's log sink. Declared non-variadic on purpose: the core calls
/// it with a printf-style varargs tail, which this ignores (it prints the
/// format string). On arm64 the extra arguments sit in registers/stack the
/// callee never reads, so the mismatch is harmless.
unsafe extern "C" fn core_log(level: c_uint, fmt: *const c_char) {
    if fmt.is_null() {
        return;
    }
    let text = CStr::from_ptr(fmt).to_string_lossy();
    eprintln!("core[{level}]: {text}");
}

unsafe extern "C" fn environment_cb(cmd: c_uint, data: *mut c_void) -> bool {
    match current() {
        Some(host) => host.environment(cmd, data),
        None => false,
    }
}

unsafe extern "C" fn video_cb(data: *const c_void, width: c_uint, height: c_uint, pitch: usize) {
    if let Some(host) = current() {
        host.on_video(data, width, height, pitch);
    }
}

unsafe extern "C" fn audio_cb(left: i16, right: i16) {
    if let Some(host) = current() {
        host.on_audio_batch(&[left, right]);
    }
}

unsafe extern "C" fn audio_batch_cb(data: *const i16, frames: usize) -> usize {
    let Some(host) = current() else {
        return 0;
    };
    if data.is_null() {
        return 0;
    }
    let samples = std::slice::from_raw_parts(data, frames * 2);
    host.on_audio_batch(samples);
    frames
}

unsafe extern "C" fn input_poll_cb() {
    // The host already knows the button state; polling is a no-op.
}

unsafe extern "C" fn input_state_cb(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    let Some(host) = current() else {
        return 0;
    };
    match device {
        RETRO_DEVICE_JOYPAD => {
            let mask = host
                .input
                .get(port as usize)
                .map(|slot| slot.load(Ordering::Relaxed))
                .unwrap_or(0);
            if id == RETRO_DEVICE_ID_JOYPAD_MASK {
                return mask as i16;
            }
            if id > 15 {
                return 0;
            }
            ((mask >> id) & 1) as i16
        }
        RETRO_DEVICE_ANALOG => {
            let stick = match index {
                RETRO_DEVICE_INDEX_ANALOG_LEFT => 0,
                RETRO_DEVICE_INDEX_ANALOG_RIGHT => 1,
                _ => return 0,
            };
            let axis = match id {
                RETRO_DEVICE_ID_ANALOG_X => 0,
                RETRO_DEVICE_ID_ANALOG_Y => 1,
                _ => return 0,
            };
            host.analog
                .get(port as usize)
                .and_then(|port| port.get(stick * 2 + axis))
                .map(|slot| slot.load(Ordering::Relaxed))
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Copy a null-terminated `retro_input_descriptor` array into owned values.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_INPUT_DESCRIPTORS`.
unsafe fn read_input_descriptors(data: *mut c_void) -> Vec<InputDescriptor> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    let mut cursor = data as *const retro_input_descriptor;
    loop {
        let entry = &*cursor;
        if entry.description.is_null() {
            break;
        }
        out.push(InputDescriptor {
            port: entry.port,
            device: entry.device,
            index: entry.index,
            id: entry.id,
            description: CStr::from_ptr(entry.description)
                .to_string_lossy()
                .into_owned(),
        });
        cursor = cursor.add(1);
    }
    out
}

/// A lock that survives poisoning: a panic in one callback must not make every
/// later frame panic at the `.lock().unwrap()`.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A `const char*` from a core as an owned `String` (empty when null).
fn cstring(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

fn scale5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn scale6(value: u8) -> u8 {
    (value << 2) | (value >> 4)
}
