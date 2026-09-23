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
mod png_codec;
mod saves;
mod settings;

pub use cores::load_cores;
pub use error::LibraryError;
pub use library::{collect_games, scan_dir, DiskGame, Game, Library, Screenshot};
pub use paths::{battery_save_path, save_state_path, save_state_thumb_path, seed_dir, Paths};
pub use png_codec::{decode_png, encode_png};
pub use saves::{exists, list_slots, read, remove, write, StateSlot, SLOT_COUNT};
pub use settings::Settings;
