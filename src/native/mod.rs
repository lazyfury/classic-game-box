//! The embedded native host, shared by the Swift/macOS and C++/Win32 shells.
//!
//! Both shells do the same thing: own a window, hand Rust an opaque handle, ask
//! for frames and forward native events. Everything else — the igui UI, the
//! wgpu renderer, the emulator, and the C ABI the shell talks to — is here once.
//!
//! ```text
//! macOS: AppKit + CAMetalLayer + NSEvent   ─┐
//! Windows: Win32 + HWND + WndProc + XInput ─┤─ cgb_host_* ─▶ cgb-app (UI + libretro)
//! ```
//!
//! The only platform-specific pieces are [`surface::create_surface`] (the wgpu
//! surface target) and [`input::key_from_code`] (the shell's virtual-key
//! vocabulary). The shells themselves stay separate — AppKit is Swift, Win32 is
//! C++ — but they speak one ABI.

pub mod input;

mod ffi;
mod gpu;
mod plugins;
mod surface;

pub use ffi::*;
pub use gpu::{NativeGpu, NativeGpuPlugin};
pub use input::{NativeEvent, NativeInputPlugin};
pub use plugins::{NativeClipboardPlugin, NativeGamepadPlugin, NativeTextMeasurePlugin};
pub use surface::NativeSurface;
