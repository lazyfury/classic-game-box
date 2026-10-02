//! The game library and its games' files: the SQLite database, ROM import and
//! screenshots, save states / battery saves, and cheats.
//!
//! Deliberately free of UI and of libretro. Where files live is `cgb-paths`;
//! the `cores.json` manifest and the core catalog/downloader are `cgb-cores`.

mod cheats;
mod db;
mod error;
mod import;
mod png_codec;
mod saves;
mod schema;

pub use cheats::{load_cheats, parse_cht, save_cheats, write_cht, Cheat};
pub use db::{
    collect_games, scan_cheats, scan_dir, scan_saves, DiskCheat, DiskGame, DiskSave, Game, Library,
    SaveKind, Screenshot,
};
pub use error::LibraryError;
pub use import::{import_roms, move_into, ImportReport};
pub use png_codec::{decode_png, encode_png};
pub use saves::{
    compact_quick, exists, list_quick_slots, list_slots, read, remove, remove_legacy_quick,
    roll_quick, write, StateSlot, MANUAL_SLOT_COUNT, QUICK_SLOT_COUNT,
};
