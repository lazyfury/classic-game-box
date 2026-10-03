//! Core metadata: the `cores.json` manifest, the downloadable-core catalog and
//! the runtime downloader.
//!
//! Deliberately free of UI, of libretro and of the game database. The manifest
//! is data (`cores/cores.json`); the catalog is the libretro buildbot's core
//! list, cached locally so search works offline.

mod catalog;
mod download;
mod manifest;

pub use catalog::{
    cache_path, is_blocked, is_unstable, registry_path, Catalog, CatalogEntry, Platform,
    BLOCKED_CORES, DEFAULT_SOURCE, UNSTABLE_CORES,
};
pub use download::{
    download_core, download_core_with_progress, register_downloaded, update_catalog, write_catalog,
    DownloadError,
};
pub use manifest::load_cores;
