//! The library, settings and saves: everything that outlives one run.
//!
//! Deliberately free of UI and of libretro. It answers three questions:
//! where files live ([`Paths`]), which ROMs are known ([`Library`],
//! [`scan_dir`]), and how to read and write a save
//! ([`saves`], [`save_state_path`], [`battery_save_path`]).

mod cheats;
mod cores;
mod error;
mod import;
mod library;
mod paths;
mod png_codec;
mod saves;
mod settings;

pub use cheats::{load_cheats, parse_cht, save_cheats, write_cht, Cheat};
pub use cores::load_cores;
pub use error::LibraryError;
pub use import::{import_roms, ImportReport};
pub use library::{collect_games, scan_dir, DiskGame, Game, Library, Screenshot};
pub use paths::{
    battery_save_path, cheat_file, save_state_path, save_state_thumb_path, seed_dir,
    seed_dir_recursive, Paths,
};
pub use png_codec::{decode_png, encode_png};
pub use saves::{exists, list_slots, read, remove, write, StateSlot, SLOT_COUNT};
pub use settings::Settings;
