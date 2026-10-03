//! The libretro front end: load a native core, register its callbacks, run it.
//!
//! This crate is the only place that knows libretro. It depends on
//! `cgb-systems` for the shared joypad ids and on `libloading` for `dlopen` —
//! and on nothing else. In particular it does **not** depend on the UI or on
//! the audio device: it exposes plain values ([`Frame`], `Vec<i16>` samples)
//! that the app drains each frame. That keeps the UI headless-testable and the
//! core host free of platform specifics.
//!
//! ```no_run
//! use cgb_libretro::CoreHost;
//! # fn main() -> Result<(), cgb_libretro::LibretroError> {
//! let mut core = CoreHost::new(
//!     "cores/dist/mesen_libretro.dylib",
//!     "/tmp/cgb/system",
//!     "/tmp/cgb/saves",
//! )?;
//! let rom = std::fs::read("mario.nes").unwrap();
//! core.load_game("mario.nes", &rom)?;
//! let av = core.av_info();
//! core.run_frame();
//! if let Some(frame) = core.take_frame() {
//!     assert_eq!((frame.width, frame.height), (av.width, av.height));
//! }
//! # Ok(())
//! # }
//! ```

pub mod ffi;

#[cfg(target_os = "macos")]
mod gl;

/// Hardware rendering is only implemented on macOS (offscreen CGL). Elsewhere
/// the module exists so `host` compiles, but every context request is refused.
#[cfg(not(target_os = "macos"))]
mod gl {
    use std::ffi::{c_void, CStr};

    pub const DEFAULT_WIDTH: u32 = 640;
    pub const DEFAULT_HEIGHT: u32 = 480;

    /// Never constructed: [`GlContext::new`] always fails off macOS.
    pub struct GlContext {
        width: u32,
        height: u32,
    }

    impl GlContext {
        pub fn new(
            _width: u32,
            _height: u32,
            _depth: bool,
            _stencil: bool,
            _flip: bool,
        ) -> Result<Self, String> {
            Err("hardware rendering is not implemented on this platform".to_string())
        }

        pub fn framebuffer(&self) -> u32 {
            0
        }

        pub fn make_current(&self) -> Result<(), String> {
            Err("hardware rendering is not implemented on this platform".to_string())
        }

        pub fn read_frame(
            &mut self,
            _width: u32,
            _height: u32,
            _depth: bool,
            _stencil: bool,
        ) -> Result<Vec<u8>, String> {
            let _ = (self.width, self.height);
            Err("hardware rendering is not implemented on this platform".to_string())
        }
    }

    pub fn proc_address(_name: &CStr) -> *mut c_void {
        std::ptr::null_mut()
    }

    /// No-op counterpart of the macOS `glBindFramebuffer` wrapper.
    ///
    /// # Safety
    ///
    /// Has the same contract as the real entry point, but there is never a
    /// live hardware context on this platform, so the call does nothing.
    pub unsafe fn bind_framebuffer(_target: u32, _framebuffer: u32) {}
}

mod error;
mod host;
mod loader;

pub use error::LibretroError;
pub use host::{AvInfo, CoreHost, CoreOption, Frame, InputDescriptor, MemoryRegion, SystemInfo};
pub use loader::CoreLibrary;

mod core_choice;
mod joypad;
mod system;

pub mod input;
pub use core_choice::{choose_core, cores_for_system, CoreSpec};
pub use input::{
    default_key_hints, GamepadSnapshot, InputState, Key, KeyboardBindings, KeyboardMode, MAX_PORTS,
};
pub use joypad::{
    JoypadButton, RETRO_DEVICE_ANALOG, RETRO_DEVICE_ANALOG_BIT, RETRO_DEVICE_ID_ANALOG_X,
    RETRO_DEVICE_ID_ANALOG_Y, RETRO_DEVICE_ID_JOYPAD_MASK, RETRO_DEVICE_INDEX_ANALOG_LEFT,
    RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_JOYPAD, RETRO_DEVICE_JOYPAD_BIT,
};
pub use system::{extension_of, system_for_path, SystemId, SYSTEMS};
