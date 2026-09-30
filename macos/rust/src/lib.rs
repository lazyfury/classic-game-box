//! The embedded macOS host for `cgb-app`, exposed over a C ABI.
//!
//! Swift owns the window and its `CAMetalLayer`; this crate owns the rest —
//! the igui UI, the wgpu renderer and the emulator — and drives the runtime
//! from frames Swift asks for. That is the whole point: the FFI boundary
//! includes the UI, so Swift never paints it.
//!
//! ```text
//! Swift: NSWindow + CAMetalLayer + native events
//!   │  layer*, tick, pointer/key events
//!   ▼
//! cgb-mac: igui_app runtime + igui UI + cgb-app (libretro + audio)
//! ```

mod ffi;
mod host;
mod input;

pub use ffi::*;
pub use host::{MacGpu, MacGpuPlugin, MacSurface, MacTextMeasurePlugin};
pub use input::MacInputPlugin;
