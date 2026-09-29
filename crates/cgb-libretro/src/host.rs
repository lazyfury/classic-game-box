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
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI16, AtomicPtr, AtomicU16, AtomicU32, Ordering};
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

/// One core option, owned, for the settings UI.
#[derive(Clone, Debug)]
pub struct CoreOption {
    pub key: String,
    pub label: String,
    /// `(value, label)` pairs the option accepts.
    pub values: Vec<(String, String)>,
    /// The current value.
    pub value: String,
}

/// A core option plus a `CString` for its value, so `GET_VARIABLE` can hand the
/// core a pointer that stays valid until the value changes.
struct HostOption {
    key: String,
    label: String,
    values: Vec<(String, String)>,
    value: String,
    value_c: CString,
}

/// Hardware-rendering state: whether a GL core is active, plus the destroy
/// callback and framebuffer flags it asked for. The `GlContext` itself lives in
/// [`HostShared::gl`].
#[derive(Clone, Copy, Default)]
struct HwRenderState {
    /// The core's `context_reset`, called once `retro_load_game` has returned
    /// and the offscreen GL context and FBO exist. Deferred on purpose: a core
    /// may only arm its first-reset hook at the *end* of `retro_load_game`
    /// (Mupen64Plus-Next sets `first_context_reset` there), so calling it from
    /// inside `SET_HW_RENDER` leaves the graphics plugin unconnected.
    reset: Option<RetroHwContextResetFn>,
    /// The core's `context_destroy`, called just before the context is torn
    /// down.
    destroy: Option<RetroHwContextResetFn>,
    /// Whether depth / stencil attachments were requested (reused when the FBO
    /// is rebuilt for a new size).
    depth: bool,
    stencil: bool,
    /// True once `SET_HW_RENDER` was accepted and the context is live.
    active: bool,
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
    /// Core options from `SET_VARIABLES` / `SET_CORE_OPTIONS`.
    core_options: Mutex<Vec<HostOption>>,
    /// Set when an option value changed; cleared by `GET_VARIABLE_UPDATE`.
    options_dirty: AtomicBool,
    /// The last `SET_MESSAGE` text, taken by the app.
    message: Mutex<Option<String>>,
    /// Hardware-rendering state (GL core), or defaults when none is active.
    hw: Mutex<HwRenderState>,
    /// The offscreen GL context and framebuffer, when a core asked for one.
    gl: Mutex<Option<crate::gl::GlContext>>,
    /// Set of the frame size the core reported through
    /// `RETRO_HW_FRAME_BUFFER_VALID`, pending a read-back in `take_frame`.
    hw_frame: Mutex<Option<(u32, u32)>>,
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
            core_options: Mutex::new(Vec::new()),
            options_dirty: AtomicBool::new(false),
            message: Mutex::new(None),
            hw: Mutex::new(HwRenderState::default()),
            gl: Mutex::new(None),
            hw_frame: Mutex::new(None),
        }
    }

    /// Copy one frame and swizzle it to RGBA8.
    ///
    /// # Safety
    ///
    /// `data` points to `pitch * height` readable bytes in the core's frame
    /// format, as libretro promises for the duration of the callback.
    unsafe fn on_video(&self, data: *const c_void, width: u32, height: u32, pitch: usize) {
        // A hardware-rendering core hands over a sentinel, not pixels: the image
        // is in the GL framebuffer and is read back in `take_frame`. Do not
        // dereference `data`.
        if data as usize == RETRO_HW_FRAME_BUFFER_VALID {
            *lock(&self.hw_frame) = Some((width, height));
            return;
        }
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

    /// Accept a hardware-rendering request: build an offscreen GL context and
    /// FBO, hand the core the two front-end callbacks, then let it create its
    /// resources by calling its `context_reset`.
    ///
    /// Only desktop OpenGL is satisfiable here; every other context type is
    /// refused so the core can fall back (or report the failure).
    ///
    /// # Safety
    ///
    /// `data` is the `retro_hw_render_callback *` from `SET_HW_RENDER`.
    unsafe fn set_hw_render(&self, data: *mut c_void) -> bool {
        if data.is_null() {
            return false;
        }
        let callback = &mut *(data as *mut retro_hw_render_callback);
        if callback.context_type != RETRO_HW_CONTEXT_OPENGL_CORE
            && callback.context_type != RETRO_HW_CONTEXT_OPENGL
        {
            return false;
        }

        let depth = callback.depth;
        let stencil = callback.stencil;
        let flip = callback.bottom_left_origin;

        let context = match crate::gl::GlContext::new(
            crate::gl::DEFAULT_WIDTH,
            crate::gl::DEFAULT_HEIGHT,
            depth,
            stencil,
            flip,
        ) {
            Ok(context) => context,
            Err(error) => {
                eprintln!("cgb-libretro: hardware render unavailable: {error}");
                return false;
            }
        };
        if let Err(error) = context.make_current() {
            eprintln!("cgb-libretro: hardware render unavailable: {error}");
            return false;
        }

        // Fill in the front end's half of the contract before the core uses
        // any of it.
        callback.get_current_framebuffer = Some(get_current_framebuffer_cb);
        callback.get_proc_address = Some(get_proc_address_cb);

        let reset = callback.context_reset;
        let destroy = callback.context_destroy;

        *lock(&self.gl) = Some(context);
        {
            let mut hw = lock(&self.hw);
            hw.reset = reset;
            hw.destroy = destroy;
            hw.depth = depth;
            hw.stencil = stencil;
            hw.active = true;
        }

        // `context_reset` is deliberately *not* called here; see `HwRenderState`.
        // It runs from `CoreHost::load_game` once `retro_load_game` returns.
        true
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
                (*callback).log = cgb_core_log as *const () as *mut c_void;
                true
            }
            RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE => {
                if data.is_null() {
                    return false;
                }
                // Same shape of core bug as the log interface above: some
                // cores (FreeJ2ME-Plus) call `set_rumble_state`
                // unconditionally once they hold the interface, so declining
                // leaves them calling a null pointer. Hand back a real no-op.
                let interface = data as *mut retro_rumble_interface;
                (*interface).set_rumble_state = Some(set_rumble_state);
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
            // Core options: we speak v2, so a modern core uses
            // `SET_CORE_OPTIONS_V2` (with categories), a v1 core uses
            // `SET_CORE_OPTIONS`, and an older one falls back to
            // `SET_VARIABLES`. FreeJ2ME-Plus only gets this right at v2: at v1
            // it hands a v2 array to `SET_CORE_OPTIONS`, which would misread.
            RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => {
                if !data.is_null() {
                    *(data as *mut c_uint) = 2;
                }
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
                *lock(&self.core_options) = read_core_options(data);
                self.options_dirty.store(true, Ordering::Relaxed);
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 => {
                *lock(&self.core_options) = read_core_options_v2(data);
                self.options_dirty.store(true, Ordering::Relaxed);
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL => {
                *lock(&self.core_options) = read_core_options_intl(data);
                self.options_dirty.store(true, Ordering::Relaxed);
                true
            }
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => {
                *lock(&self.core_options) = read_core_options_v2_intl(data);
                self.options_dirty.store(true, Ordering::Relaxed);
                true
            }
            RETRO_ENVIRONMENT_GET_VARIABLE => {
                if data.is_null() {
                    return false;
                }
                let variable = &mut *(data as *mut retro_variable);
                if variable.key.is_null() {
                    return false;
                }
                let key = cstring(variable.key);
                let options = lock(&self.core_options);
                match options.iter().find(|option| option.key == key) {
                    Some(option) => {
                        variable.value = option.value_c.as_ptr();
                        true
                    }
                    None => {
                        variable.value = ptr::null();
                        false
                    }
                }
            }
            RETRO_ENVIRONMENT_SET_VARIABLES => {
                *lock(&self.core_options) = read_variables(data);
                self.options_dirty.store(true, Ordering::Relaxed);
                true
            }
            RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                if !data.is_null() {
                    *(data as *mut bool) = self.options_dirty.swap(false, Ordering::Relaxed);
                }
                true
            }
            // Message interface: the last message goes to the app's status line.
            RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION => {
                if !data.is_null() {
                    *(data as *mut c_uint) = 1;
                }
                true
            }
            RETRO_ENVIRONMENT_SET_MESSAGE => {
                if !data.is_null() {
                    let message = &*(data as *const retro_message);
                    *lock(&self.message) = Some(cstring(message.msg));
                }
                true
            }
            RETRO_ENVIRONMENT_SET_MESSAGE_EXT => {
                if !data.is_null() {
                    let message = &*(data as *const retro_message_ext);
                    *lock(&self.message) = Some(cstring(message.msg));
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
            | RETRO_ENVIRONMENT_SET_GEOMETRY => true,
            // Hardware rendering: we can offer OpenGL, so a core that wants it
            // gets a context; the preference hint steers cores that ask.
            RETRO_ENVIRONMENT_SET_HW_RENDER => self.set_hw_render(data),
            RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER => {
                if !data.is_null() {
                    *(data as *mut c_int) = RETRO_HW_CONTEXT_OPENGL_CORE;
                }
                true
            }
            // Everything else (core options callbacks, VFS, …) is reported
            // unsupported so the core degrades predictably.
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

        // A hardware-rendering core builds its GL resources on the first
        // context reset. That must run *now*, after `retro_load_game` has
        // returned: the core only arms its first-reset hook at the end of
        // load_game (this is why the front end must not reset from inside
        // `SET_HW_RENDER`). The GL context is still current and the FBO exists.
        let reset = lock(&self.shared.hw).reset.take();
        if let Some(reset) = reset {
            unsafe { reset() };
        }
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
    ///
    /// Software cores leave their pixels in `video`; a hardware core leaves a
    /// size marker and the image in the GL framebuffer, so read that back here
    /// on the calling (main) thread.
    pub fn take_frame(&self) -> Option<Frame> {
        if let Some(frame) = lock(&self.shared.video).take() {
            return Some(frame);
        }
        let (width, height) = lock(&self.shared.hw_frame).take()?;
        let (depth, stencil) = {
            let hw = lock(&self.shared.hw);
            (hw.depth, hw.stencil)
        };
        let mut guard = lock(&self.shared.gl);
        let context = guard.as_mut()?;
        let width = width.max(1);
        let height = height.max(1);
        match context.read_frame(width, height, depth, stencil) {
            Ok(rgba) => Some(Frame {
                width,
                height,
                rgba,
            }),
            Err(error) => {
                eprintln!("cgb-libretro: frame read-back failed: {error}");
                None
            }
        }
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

    /// The core's options, owned, for the settings UI.
    pub fn core_options(&self) -> Vec<CoreOption> {
        lock(&self.shared.core_options)
            .iter()
            .map(|option| CoreOption {
                key: option.key.clone(),
                label: option.label.clone(),
                values: option.values.clone(),
                value: option.value.clone(),
            })
            .collect()
    }

    /// Set a core option value; the core sees it on the next `GET_VARIABLE`.
    pub fn set_core_option(&self, key: &str, value: &str) {
        let mut options = lock(&self.shared.core_options);
        if let Some(option) = options.iter_mut().find(|option| option.key == key) {
            if option.value == value {
                return;
            }
            option.value = value.to_string();
            option.value_c = CString::new(value).unwrap_or_default();
            self.shared.options_dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Take the last message the core pushed, if any.
    pub fn take_message(&self) -> Option<String> {
        lock(&self.shared.message).take()
    }

    /// Tell the core which device class a port uses; some cores require it
    /// before they answer input.
    pub fn set_controller_port_device(&self, port: u32, device: u32) {
        unsafe { (self.core.api().set_controller_port_device)(port as c_uint, device as c_uint) };
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

    /// Disable every cheat. Call before applying a game's cheat list.
    pub fn reset_cheats(&self) {
        unsafe { (self.core.api().cheat_reset)() };
    }

    /// Enable or disable one cheat by index. The code string's syntax is the
    /// core's (GameShark, GameGenie, RAW, …).
    pub fn set_cheat(&self, index: usize, enabled: bool, code: &str) {
        let Ok(code) = CString::new(code) else {
            return;
        };
        unsafe { (self.core.api().cheat_set)(index as c_uint, enabled, code.as_ptr()) };
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
            // Tear down the core's hardware-render resources *before*
            // `retro_unload_game`: the core expects `context_destroy` while its
            // game state and our GL context are still alive. RetroArch does the
            // same — `video_driver_free_hw_context()` (which calls
            // `context_destroy`) runs before `retro_unload_game()` in
            // `core_unload_game` (runloop.c). PPSSPP in particular deletes its
            // graphics context inside `retro_unload_game`, so calling
            // `context_destroy` afterwards null-derefs and crashes.
            let destroy = {
                let mut hw = lock(&self.shared.hw);
                let destroy = hw.destroy.take();
                hw.active = false;
                destroy
            };
            if let Some(destroy) = destroy {
                destroy();
            }
            // Drop the offscreen context and FBO once the core has released
            // its GL objects, but keep it alive across `unload_game` (a core
            // may still touch GL while shutting its renderer down).
            if self.loaded {
                (self.core.api().unload_game)();
            }
            *lock(&self.shared.gl) = None;
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

// The core's log callback is C-variadic, which Rust cannot define on stable.
// `src/log_shim.c` is the real callback: it `vsnprintf`s the message and calls
// `cgb_log_emit` with the finished line, so the log shows the actual text
// instead of the raw format string (e.g. `[%s] %s`).
extern "C" {
    fn cgb_core_log(level: c_uint, fmt: *const c_char, ...);
}

/// The Rust end of [`cgb_core_log`]: print an already-formatted log line.
///
/// # Safety
///
/// `text` is a NUL-terminated string owned by the shim, valid for this call.
#[no_mangle]
unsafe extern "C" fn cgb_log_emit(level: c_uint, text: *const c_char) {
    if text.is_null() {
        return;
    }
    let text = CStr::from_ptr(text).to_string_lossy();
    eprintln!("core[{level}]: {text}");
}

/// `retro_rumble_interface.set_rumble_state`: accept and ignore. Real gamepad
/// rumble is future work; what matters here is that the pointer is non-null.
unsafe extern "C" fn set_rumble_state(_port: c_uint, _effect: c_uint, _strength: u16) -> bool {
    true
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

/// `retro_hw_get_current_framebuffer_t`: the FBO id the core renders into.
unsafe extern "C" fn get_current_framebuffer_cb() -> usize {
    match current() {
        Some(host) => lock(&host.gl)
            .as_ref()
            .map(|context| context.framebuffer() as usize)
            .unwrap_or(0),
        None => 0,
    }
}

/// `retro_hw_get_proc_address_t`: resolve a GL symbol for the core.
unsafe extern "C" fn get_proc_address_cb(symbol: *const c_char) -> *mut c_void {
    if symbol.is_null() {
        return ptr::null_mut();
    }
    let name = CStr::from_ptr(symbol);
    // Redirect the core's "default framebuffer" (GL id 0) to the offscreen FBO
    // the front end owns. Our CGL context has no drawable, so real FBO 0 is not
    // a valid render target; a core that binds 0 (GLideN64 does) would otherwise
    // render into nothing.
    if name.to_bytes() == b"glBindFramebuffer" {
        return shim_bind_framebuffer as *mut c_void;
    }
    crate::gl::proc_address(name)
}

/// The front end FBO id, or 0 when no hardware context is live.
fn frontend_framebuffer() -> usize {
    match current() {
        Some(host) => lock(&host.gl)
            .as_ref()
            .map(|context| context.framebuffer() as usize)
            .unwrap_or(0),
        None => 0,
    }
}

/// `glBindFramebuffer` interposer: map a bind of the default framebuffer (0) to
/// the front end's FBO, then call the real function.
unsafe extern "C" fn shim_bind_framebuffer(target: c_uint, framebuffer: c_uint) {
    let remapped = if framebuffer == 0 {
        frontend_framebuffer()
    } else {
        framebuffer as usize
    };
    crate::gl::bind_framebuffer(target, remapped as c_uint);
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

/// Copy a core-options v1 array into owned options.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_CORE_OPTIONS`.
unsafe fn read_core_options(data: *mut c_void) -> Vec<HostOption> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    let mut cursor = data as *const retro_core_option_definition;
    loop {
        let definition = &*cursor;
        if definition.key.is_null() {
            break;
        }
        let key = cstring(definition.key);
        let label = cstring(definition.desc);
        let default = cstring(definition.default_value);
        let mut values = Vec::new();
        for value in definition.values.iter() {
            if value.value.is_null() {
                break;
            }
            values.push((cstring(value.value), cstring(value.label)));
        }
        out.push(host_option(key, label, values, default));
        cursor = cursor.add(1);
    }
    out
}

/// Copy a core-options v2 set (`SET_CORE_OPTIONS_V2`) into owned options.
///
/// Categories are dropped: the settings UI shows one flat list, so an option's
/// non-categorized label (`desc`) is what it shows. Cores that define v2
/// options only publish them here.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_CORE_OPTIONS_V2`.
unsafe fn read_core_options_v2(data: *mut c_void) -> Vec<HostOption> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    let options = &*(data as *const retro_core_options_v2);
    if options.definitions.is_null() {
        return out;
    }
    let mut cursor = options.definitions;
    loop {
        let definition = &*cursor;
        if definition.key.is_null() {
            break;
        }
        let key = cstring(definition.key);
        let mut label = cstring(definition.desc);
        if label.is_empty() {
            label = cstring(definition.desc_categorized);
        }
        let default = cstring(definition.default_value);
        let mut values = Vec::new();
        for value in definition.values.iter() {
            if value.value.is_null() {
                break;
            }
            values.push((cstring(value.value), cstring(value.label)));
        }
        out.push(host_option(key, label, values, default));
        cursor = cursor.add(1);
    }
    out
}

/// Copy a v1 international set (`SET_CORE_OPTIONS_INTL`), preferring the
/// English (`us`) definitions; `local` is only a translation of them.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_CORE_OPTIONS_INTL`.
unsafe fn read_core_options_intl(data: *mut c_void) -> Vec<HostOption> {
    if data.is_null() {
        return Vec::new();
    }
    let intl = &*(data as *const retro_core_options_intl);
    let definitions = if intl.us.is_null() {
        intl.local
    } else {
        intl.us
    };
    if definitions.is_null() {
        return Vec::new();
    }
    read_core_options(definitions as *mut c_void)
}

/// Copy a v2 international set (`SET_CORE_OPTIONS_V2_INTL`), preferring the
/// English (`us`) definitions.
///
/// A translations-enabled core (PPSSPP) registers its options through this
/// command, not `SET_CORE_OPTIONS_V2`, whenever the front end answers v2.
/// Missing it leaves every option unset — and a core that reads its render
/// resolution from an option then renders nothing at all.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_CORE_OPTIONS_V2_INTL`.
unsafe fn read_core_options_v2_intl(data: *mut c_void) -> Vec<HostOption> {
    if data.is_null() {
        return Vec::new();
    }
    let intl = &*(data as *const retro_core_options_v2_intl);
    let set = if intl.us.is_null() {
        intl.local
    } else {
        intl.us
    };
    if set.is_null() {
        return Vec::new();
    }
    read_core_options_v2(set as *mut c_void)
}

/// Copy a core-options v0 array (`SET_VARIABLES`) into owned options.
///
/// Each `value` is `"Description; value1|value2|…"`; the first value is the
/// default, as RetroArch treats it.
///
/// # Safety
///
/// `data` is the pointer libretro passed with `SET_VARIABLES`.
unsafe fn read_variables(data: *mut c_void) -> Vec<HostOption> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    let mut cursor = data as *const retro_variable;
    loop {
        let variable = &*cursor;
        if variable.key.is_null() {
            break;
        }
        let key = cstring(variable.key);
        let raw = cstring(variable.value);
        let (label, list) = raw
            .split_once(';')
            .map(|(desc, values)| (desc.trim().to_string(), values.trim()))
            .unwrap_or_else(|| (key.clone(), raw.as_str()));
        let mut values = Vec::new();
        let mut default = String::new();
        for (index, item) in list.split('|').enumerate() {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if index == 0 {
                default = item.to_string();
            }
            values.push((item.to_string(), item.to_string()));
        }
        out.push(host_option(key, label, values, default));
        cursor = cursor.add(1);
    }
    out
}

/// Build a [`HostOption`], keeping a `CString` of the value alive.
fn host_option(
    key: String,
    label: String,
    values: Vec<(String, String)>,
    value: String,
) -> HostOption {
    let value_c = CString::new(value.clone()).unwrap_or_default();
    HostOption {
        key,
        label,
        values,
        value,
        value_c,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostShared {
        HostShared::new(Path::new("/tmp"), Path::new("/tmp"))
    }

    #[test]
    fn the_hw_sentinel_is_not_dereferenced() {
        let host = host();
        unsafe {
            host.on_video(RETRO_HW_FRAME_BUFFER_VALID as *const c_void, 640, 480, 0);
        }
        assert!(
            lock(&host.video).is_none(),
            "no software pixels are produced"
        );
        assert_eq!(*lock(&host.hw_frame), Some((640, 480)));
    }

    #[test]
    fn preferred_hw_render_is_opengl_core() {
        let host = host();
        let mut context_type: c_int = -1;
        let ok = unsafe {
            host.environment(
                RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER,
                &mut context_type as *mut c_int as *mut c_void,
            )
        };
        assert!(ok);
        assert_eq!(context_type, RETRO_HW_CONTEXT_OPENGL_CORE);
    }

    #[test]
    fn hardware_render_refuses_vulkan() {
        let host = host();
        let mut callback = retro_hw_render_callback {
            context_type: RETRO_HW_CONTEXT_VULKAN,
            ..Default::default()
        };
        let ok = unsafe {
            host.environment(
                RETRO_ENVIRONMENT_SET_HW_RENDER,
                &mut callback as *mut retro_hw_render_callback as *mut c_void,
            )
        };
        assert!(!ok, "Vulkan is not offered");
        assert!(!lock(&host.hw).active);
    }

    #[test]
    fn v2_intl_core_options_are_parsed() {
        // PPSSPP (a translations-enabled core) registers its options through
        // `SET_CORE_OPTIONS_V2_INTL`, not `SET_CORE_OPTIONS_V2`. If this is
        // dropped every option is unset and the core's own resolution
        // default (0) makes it render nothing.
        let key = CString::new("ppsspp_internal_resolution").unwrap();
        let desc = CString::new("Internal Resolution").unwrap();
        let default = CString::new("480x272").unwrap();
        let value = CString::new("480x272").unwrap();
        let value_label = CString::new("480x272 (1x)").unwrap();

        let mut definition: retro_core_option_v2_definition = unsafe { std::mem::zeroed() };
        definition.key = key.as_ptr();
        definition.desc = desc.as_ptr();
        definition.default_value = default.as_ptr();
        definition.values[0].value = value.as_ptr();
        definition.values[0].label = value_label.as_ptr();

        let definitions = [definition, unsafe { std::mem::zeroed() }];
        let mut set = retro_core_options_v2 {
            categories: ptr::null_mut(),
            definitions: definitions.as_ptr() as *mut retro_core_option_v2_definition,
        };
        let mut intl = retro_core_options_v2_intl {
            us: &mut set,
            local: ptr::null_mut(),
        };

        let host = host();
        let ok = unsafe {
            host.environment(
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL,
                &mut intl as *mut retro_core_options_v2_intl as *mut c_void,
            )
        };
        assert!(ok);
        let options = lock(&host.core_options);
        let option = options
            .iter()
            .find(|option| option.key == "ppsspp_internal_resolution")
            .expect("the us set is parsed");
        assert_eq!(option.value, "480x272");
    }
}
