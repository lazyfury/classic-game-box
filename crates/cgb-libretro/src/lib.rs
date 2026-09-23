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

mod error;
mod host;
mod loader;

pub use error::LibretroError;
pub use host::{AvInfo, CoreHost, CoreOption, Frame, InputDescriptor, SystemInfo};
pub use loader::CoreLibrary;
