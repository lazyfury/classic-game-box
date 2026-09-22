//! Errors from loading or driving a libretro core.

use std::path::PathBuf;

/// Something the core host could not do.
#[derive(Debug, thiserror::Error)]
pub enum LibretroError {
    /// The `.dylib` could not be opened.
    #[error("could not open core {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },

    /// A required `retro_*` symbol is missing from the core.
    #[error("core {path} is missing `{symbol}`: {source}")]
    Symbol {
        path: PathBuf,
        symbol: String,
        #[source]
        source: libloading::Error,
    },

    /// The core reports an ABI version this host does not speak.
    #[error("core ABI version {found} is not supported (expected {expected})")]
    AbiVersion { found: u32, expected: u32 },

    /// `retro_load_game` returned false.
    #[error("the core refused to load `{path}`")]
    LoadGame { path: String },

    /// `retro_load_game` did not get a byte buffer or a path.
    #[error("no ROM data or path given")]
    EmptyGame,

    /// The core produced no frame when one was required.
    #[error("the core produced no frame")]
    NoFrame,
}
