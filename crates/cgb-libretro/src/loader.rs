//! Loading a native libretro core with `dlopen`/`dlsym` (`libloading`).
//!
//! This is the whole reason the native rewrite can drop the WebAssembly side
//! module machinery: a `.dylib` is opened at runtime, its `retro_*` symbols are
//! resolved, and the front end drives it through function pointers. No
//! Emscripten, no `MAIN_MODULE`, no typed-array invalidation.

use std::os::raw::c_uint;
use std::path::{Path, PathBuf};

use libloading::Library;

use crate::error::LibretroError;
use crate::ffi::*;

/// The resolved `retro_*` entry points of one core.
///
/// Every field is a plain function pointer into the loaded library. The
/// library must outlive the `Api`; [`CoreLibrary`] keeps both together and
/// drops the pointers before the library never escapes.
///
/// A few fields (`get_region`, `set_controller_port_device`, `cheat_*`) are
/// resolved now but not called yet; keeping them here documents the ABI surface
/// this host has committed to.
#[allow(dead_code)]
pub(crate) struct Api {
    pub set_environment: FnSetEnvironment,
    pub init: FnVoid,
    pub deinit: FnVoid,
    pub set_video_refresh: FnSetVideo,
    pub set_audio_sample: FnSetAudio,
    pub set_audio_sample_batch: FnSetAudioBatch,
    pub set_input_poll: FnSetInputPoll,
    pub set_input_state: FnSetInputState,
    pub load_game: FnLoadGame,
    pub unload_game: FnVoid,
    pub run: FnVoid,
    pub reset: FnVoid,
    pub system_info: FnSystemInfo,
    pub av_info: FnAvInfo,
    pub serialize_size: FnSerializeSize,
    pub serialize: FnSerialize,
    pub unserialize: FnUnserialize,
    pub get_memory_data: FnGetMemoryData,
    pub get_memory_size: FnGetMemorySize,
    pub get_region: FnGetRegion,
    pub set_controller_port_device: FnSetControllerPortDevice,
    pub cheat_reset: FnVoid,
    pub cheat_set: FnCheatSet,
}

/// A loaded core: the `dlopen` handle and its resolved entry points.
pub struct CoreLibrary {
    api: Api,
    /// Kept last so the symbols are dropped before the handle. Function
    /// pointers are `'static` and do not borrow it, but this documents intent.
    _library: Library,
    path: PathBuf,
}

/// Resolve one required symbol, naming it in the error if it is missing.
unsafe fn symbol<T: Copy>(
    library: &Library,
    path: &Path,
    name: &'static [u8],
) -> Result<T, LibretroError> {
    let symbol = library
        .get::<T>(name)
        .map_err(|source| LibretroError::Symbol {
            path: path.to_path_buf(),
            symbol: String::from_utf8_lossy(name)
                .trim_end_matches('\0')
                .to_string(),
            source,
        })?;
    Ok(*symbol)
}

impl CoreLibrary {
    /// Open a core and resolve the `retro_*` symbols this host needs.
    ///
    /// Returns [`LibretroError::AbiVersion`] if the core speaks a different
    /// libretro ABI than this header.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LibretroError> {
        let path = path.as_ref();
        // SAFETY: the library is kept alive for as long as the function
        // pointers are used (same struct), and every symbol is resolved before
        // `open` returns.
        let library = unsafe { Library::new(path) }.map_err(|source| LibretroError::Open {
            path: path.to_path_buf(),
            source,
        })?;

        // SAFETY: every symbol resolves from the just-loaded `library`, which is stored beside the fn pointers and outlives them.
        unsafe {
            let version: FnVersion = symbol(&library, path, b"retro_api_version\0")?;
            let found = version();
            if found != RETRO_API_VERSION {
                return Err(LibretroError::AbiVersion {
                    found: found as u32,
                    expected: RETRO_API_VERSION,
                });
            }

            let api = Api {
                set_environment: symbol(&library, path, b"retro_set_environment\0")?,
                init: symbol(&library, path, b"retro_init\0")?,
                deinit: symbol(&library, path, b"retro_deinit\0")?,
                set_video_refresh: symbol(&library, path, b"retro_set_video_refresh\0")?,
                set_audio_sample: symbol(&library, path, b"retro_set_audio_sample\0")?,
                set_audio_sample_batch: symbol(&library, path, b"retro_set_audio_sample_batch\0")?,
                set_input_poll: symbol(&library, path, b"retro_set_input_poll\0")?,
                set_input_state: symbol(&library, path, b"retro_set_input_state\0")?,
                load_game: symbol(&library, path, b"retro_load_game\0")?,
                unload_game: symbol(&library, path, b"retro_unload_game\0")?,
                run: symbol(&library, path, b"retro_run\0")?,
                reset: symbol(&library, path, b"retro_reset\0")?,
                system_info: symbol(&library, path, b"retro_get_system_info\0")?,
                av_info: symbol(&library, path, b"retro_get_system_av_info\0")?,
                serialize_size: symbol(&library, path, b"retro_serialize_size\0")?,
                serialize: symbol(&library, path, b"retro_serialize\0")?,
                unserialize: symbol(&library, path, b"retro_unserialize\0")?,
                get_memory_data: symbol(&library, path, b"retro_get_memory_data\0")?,
                get_memory_size: symbol(&library, path, b"retro_get_memory_size\0")?,
                get_region: symbol(&library, path, b"retro_get_region\0")?,
                set_controller_port_device: symbol(
                    &library,
                    path,
                    b"retro_set_controller_port_device\0",
                )?,
                cheat_reset: symbol(&library, path, b"retro_cheat_reset\0")?,
                cheat_set: symbol(&library, path, b"retro_cheat_set\0")?,
            };

            Ok(Self {
                api,
                _library: library,
                path: path.to_path_buf(),
            })
        }
    }

    /// The path the core was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn api(&self) -> &Api {
        &self.api
    }
}

/// Region constants, for callers that want to know NTSC vs PAL. Declared here
/// (not used by the host yet) to keep the enum in one place.
#[allow(dead_code)]
pub(crate) const RETRO_REGION_NTSC: c_uint = 0;
#[allow(dead_code)]
pub(crate) const RETRO_REGION_PAL: c_uint = 1;
