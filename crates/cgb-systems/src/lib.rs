//! Pure domain types for Classic Game Box: which console a file is for and
//! which libretro core runs it, plus the joypad ids shared by the input and
//! libretro layers.
//!
//! This crate is deliberately dependency-free — no UI, no platform, no
//! libretro. It is the one place the (extension → console → core) mapping
//! lives, so the UI, the input layer and the core host cannot disagree about
//! it. It mirrors `electron/src/renderer/systems.ts` from the old front end,
//! minus the core that no longer exists.

mod core_choice;
mod joypad;
mod system;

pub use core_choice::{choose_core, cores_for_system, CoreSpec};
pub use joypad::{
    JoypadButton, RETRO_DEVICE_ANALOG, RETRO_DEVICE_ANALOG_BIT, RETRO_DEVICE_ID_ANALOG_X,
    RETRO_DEVICE_ID_ANALOG_Y, RETRO_DEVICE_ID_JOYPAD_MASK, RETRO_DEVICE_INDEX_ANALOG_LEFT,
    RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_JOYPAD, RETRO_DEVICE_JOYPAD_BIT,
};
pub use system::{extension_of, system_for_path, SystemId, SYSTEMS};
