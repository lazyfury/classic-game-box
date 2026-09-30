//! Core metadata: the `cores.json` manifest, the downloadable-core catalog and
//! the runtime downloader.
//!
//! Deliberately free of UI, of libretro and of the game database. The manifest
//! is data (`cores/cores.json`); the catalog is the libretro buildbot's core
//! list, cached locally so search works offline.

mod catalog;
mod cores;
mod download;

pub use catalog::{cache_path, registry_path, Catalog, CatalogEntry, Platform, DEFAULT_SOURCE};
pub use cores::load_cores;
pub use download::{
    download_core, download_core_with_progress, register_downloaded, update_catalog, write_catalog,
    DownloadError,
};
