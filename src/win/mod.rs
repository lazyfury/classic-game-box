//! The embedded Windows host for `cgb-app`, exposed over a C ABI.
//!
//! The C++ Win32 shell owns the window and its `HWND`; this module owns the
//! rest — the igui UI, the wgpu renderer and the emulator — and drives the
//! runtime from frames the shell asks for. That is the whole point: the FFI
//! boundary includes the UI, so C++ never paints it.
//!
//! ```text
//! C++: RegisterClassExW + CreateWindowExW + WndProc + XInput
//!   │  hwnd*, tick, pointer/key events
//!   ▼
//! cgb-win: igui_app runtime + igui UI + cgb-app (libretro + audio)
//! ```
//!
//! Mirrors [`crate::mac`].
//!
//! Unlike the macOS host, this module is **not** `cfg`-gated. `raw-window-handle`
//! exposes the `Win32`/`Windows` variants on every target, so the whole host
//! type-checks on the macOS dev machine — and therefore the default
//! `cargo clippy` / `cargo test` gate keeps it honest (there is no Windows
//! machine here to compile it on). At runtime it is inert unless the C++ shell
//! calls `cgb_win_start`; the macOS build never does.

pub mod input;

pub use input::WinInputPlugin;

mod ffi;
mod host;

pub use ffi::*;
pub use host::{
    WinClipboardPlugin, WinGamepadPlugin, WinGpu, WinGpuPlugin, WinSurface, WinTextMeasurePlugin,
};
