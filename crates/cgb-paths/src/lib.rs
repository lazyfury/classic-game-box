//! Paths and settings: where the app keeps files, and what it remembers.
//!
//! Deliberately free of UI and of libretro. [`Paths`] answers "where does the
//! library, the saves, the screenshots and the cores live"; [`Settings`] is the
//! small JSON document that names the chosen library and the per-console core.

mod paths;
mod settings;

pub use paths::{
    battery_save_path, cheat_file, save_state_path, save_state_thumb_path, seed_dir,
    seed_dir_recursive, Paths,
};
pub use settings::Settings;
