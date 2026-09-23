//! Hand-written bindings for the slice of the libretro ABI this front end uses.
//!
//! The full contract is vendored next to this crate (`libretro.h`, ≈8700 lines)
//! and is the source of truth. **Do not read it top to bottom** — grep a symbol
//! and read that window. Only the types and constants the host actually
//! touches are declared here, so a core that needs something we have not
//! implemented gets a documented `false` from `environment`, not a silent wild
//! pointer.
//!
//! # Safety
//!
//! Every type below is `#[repr(C)]` and laid out exactly as `libretro.h`.
//! Changing a field or its type is an ABI break; the cores are compiled
//! against the same header.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_uint, c_void};

/// The libretro ABI version this header describes (`RETRO_API_VERSION`).
pub const RETRO_API_VERSION: c_uint = 1;

// --- environment commands -------------------------------------------------
// Only the commands the host answers. Everything else returns `false`.
pub const RETRO_ENVIRONMENT_SET_ROTATION: c_uint = 1;
pub const RETRO_ENVIRONMENT_GET_OVERSCAN: c_uint = 2;
pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub const RETRO_ENVIRONMENT_SET_MESSAGE: c_uint = 6;
pub const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub const RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES: c_uint = 24;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
pub const RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION: c_uint = 59;
pub const RETRO_ENVIRONMENT_SET_MESSAGE_EXT: c_uint = 60;
/// `RETRO_ENVIRONMENT_GET_INPUT_BITMASKS` is experimental (`51 | 0x10000`).
pub const RETRO_ENVIRONMENT_EXPERIMENTAL: c_uint = 0x10000;
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;

// --- memory / pixel formats ----------------------------------------------
pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
pub const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;

/// `RETRO_PIXEL_FORMAT_*` enum values (not defines). 0RGB1555 is libretro's
/// default before a core calls `SET_PIXEL_FORMAT`.
pub const RETRO_PIXEL_FORMAT_0RGB1555: c_uint = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;

// --- structs --------------------------------------------------------------

/// `struct retro_system_info`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

/// `struct retro_game_geometry`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct retro_game_geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

/// `struct retro_system_timing`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct retro_system_timing {
    pub fps: f64,
    pub sample_rate: f64,
}

/// `struct retro_system_av_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct retro_system_av_info {
    pub geometry: retro_game_geometry,
    pub timing: retro_system_timing,
}

/// `struct retro_game_info`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct retro_game_info {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

/// `struct retro_variable`.
#[repr(C)]
pub struct retro_variable {
    pub key: *const c_char,
    pub value: *const c_char,
}

/// `struct retro_message`.
#[repr(C)]
pub struct retro_message {
    pub msg: *const c_char,
    pub frames: c_uint,
}

/// `struct retro_message_ext`.
#[repr(C)]
pub struct retro_message_ext {
    pub msg: *const c_char,
    pub duration: c_uint,
    pub priority: c_uint,
    pub level: c_uint,
    pub target: c_uint,
    pub progress: c_int,
    pub type_: c_uint,
}

/// `struct retro_core_option_value`.
#[repr(C)]
pub struct retro_core_option_value {
    pub value: *const c_char,
    pub label: *const c_char,
}

/// `RETRO_NUM_CORE_OPTION_VALUES_MAX`.
pub const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

/// `struct retro_core_option_definition` (core options v1).
///
/// The array is terminated by an entry whose `key` is null.
#[repr(C)]
pub struct retro_core_option_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

/// `struct retro_input_descriptor`.
#[repr(C)]
pub struct retro_input_descriptor {
    pub port: c_uint,
    pub device: c_uint,
    pub index: c_uint,
    pub id: c_uint,
    pub description: *const c_char,
}

/// `struct retro_controller_description`.
#[repr(C)]
pub struct retro_controller_description {
    pub desc: *const c_char,
    pub id: c_uint,
}

/// `struct retro_controller_info`.
#[repr(C)]
pub struct retro_controller_info {
    pub types: *const retro_controller_description,
    pub num_types: c_uint,
}

// --- callback types the front end gives the core --------------------------

pub type RetroEnvironmentFn = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
pub type RetroVideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
pub type RetroAudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);
pub type RetroAudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
pub type RetroInputPollFn = unsafe extern "C" fn();
pub type RetroInputStateFn =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

// --- function pointers exported by the core -------------------------------

pub type FnVersion = unsafe extern "C" fn() -> c_uint;
pub type FnSetEnvironment = unsafe extern "C" fn(RetroEnvironmentFn);
pub type FnVoid = unsafe extern "C" fn();
pub type FnSetVideo = unsafe extern "C" fn(RetroVideoRefreshFn);
pub type FnSetAudio = unsafe extern "C" fn(RetroAudioSampleFn);
pub type FnSetAudioBatch = unsafe extern "C" fn(RetroAudioSampleBatchFn);
pub type FnSetInputPoll = unsafe extern "C" fn(RetroInputPollFn);
pub type FnSetInputState = unsafe extern "C" fn(RetroInputStateFn);
pub type FnLoadGame = unsafe extern "C" fn(*const retro_game_info) -> bool;
pub type FnSystemInfo = unsafe extern "C" fn(*mut retro_system_info);
pub type FnAvInfo = unsafe extern "C" fn(*mut retro_system_av_info);
pub type FnSerializeSize = unsafe extern "C" fn() -> usize;
pub type FnSerialize = unsafe extern "C" fn(*mut c_void, usize) -> bool;
pub type FnUnserialize = unsafe extern "C" fn(*const c_void, usize) -> bool;
pub type FnGetMemoryData = unsafe extern "C" fn(c_uint) -> *mut c_void;
pub type FnGetMemorySize = unsafe extern "C" fn(c_uint) -> usize;
pub type FnGetRegion = unsafe extern "C" fn() -> c_uint;
pub type FnSetControllerPortDevice = unsafe extern "C" fn(c_uint, c_uint);
pub type FnCheatSet = unsafe extern "C" fn(c_uint, bool, *const c_char);
