//! The library, settings and saves: everything that outlives one run.
//!
//! Deliberately free of UI and of libretro. It answers three questions:
//! where files live ([`Paths`]), which ROMs are known ([`Library`],
//! [`scan_dir`]), and how to read and write a save
//! ([`saves`], [`save_state_path`], [`battery_save_path`]).

mod cores;
mod error;
mod library;
mod paths;
mod saves;
mod settings;

pub use cores::load_cores;
pub use error::LibraryError;
pub use library::{scan_dir, Game, Library};
pub use paths::{battery_save_path, save_state_path, Paths};
pub use saves::{exists, read, remove, write};
pub use settings::Settings;
