//! The downloadable-core catalog: a local copy of the libretro buildbot's core
//! list, so browsing and searching work offline.
//!
//! Two layers:
//!   * the **built-in** snapshot committed at `cores/catalog.json` and embedded
//!     at compile time, and
//!   * a **user cache** at `<app data>/cores/catalog.json`, written by
//!     `--force-update`.
//!
//! [`Catalog::load`] prefers the cache when it parses and falls back to the
//! built-in, so a fresh install (or an offline one) still has a searchable
//! list.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The default download source: the libretro nightly buildbot.
pub const DEFAULT_SOURCE: &str = "https://buildbot.libretro.com/nightly";

/// The committed snapshot of the buildbot's core list.
const BUILTIN: &str = include_str!("../../cores/catalog.json");

/// One downloadable core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// The buildbot name (`mame`, `snes9x`, …); also the module's file stem.
    pub name: String,
    /// What the UI calls it.
    #[serde(default)]
    pub display_name: String,
    /// The core-info `systemid` (`super_nes`, `mame`, …). Free-form on purpose:
    /// the catalog does not require it to be a `SystemId` key.
    #[serde(default)]
    pub system: String,
    /// The extensions the core declares (`sfc|smc|…`).
    #[serde(default)]
    pub extensions: String,
}

/// The whole list, as stored on disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    /// When the snapshot was taken (informational; a Unix timestamp on
    /// `--force-update`).
    #[serde(default)]
    pub generated: String,
    /// The base URL the list came from.
    #[serde(default)]
    pub source: String,
    /// The cores, in name order.
    pub cores: Vec<CatalogEntry>,
}

/// Cores the buildbot offers but this app deliberately hides: the module
/// downloads and loads, yet cannot run any content here, so offering it only
/// wastes a download. Kept in one place so the download catalog, the
/// per-console recommendation and the downloader all agree. See
/// cores/README.md ("Known gaps").
pub const BLOCKED_CORES: &[&str] = &["squirreljme"];

/// Whether a core name is on the blocklist (case-insensitive).
pub fn is_blocked(name: &str) -> bool {
    BLOCKED_CORES
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(name))
}

/// Cores this app can download and load, but that are known to be **unstable**:
/// they crash on save / reset / unloading / switching games, so they are not the
/// recommended pick for their console. Unlike [`BLOCKED_CORES`] they stay
/// listed, downloadable and usable — the download list just tags them, so the
/// player knows what they are choosing. Kept in one place so the UI and the
/// recommendation agree. See cores/README.md.
pub const UNSTABLE_CORES: &[&str] = &["pcsx2", "armsx2", "pcee2"];

/// Whether a core name is on the unstable list (case-insensitive).
pub fn is_unstable(name: &str) -> bool {
    UNSTABLE_CORES
        .iter()
        .any(|core| core.eq_ignore_ascii_case(name))
}

impl Catalog {
    /// The built-in snapshot, embedded at compile time.
    pub fn builtin() -> Self {
        serde_json::from_str(BUILTIN).unwrap_or_else(|_| Catalog {
            generated: String::new(),
            source: DEFAULT_SOURCE.to_string(),
            cores: Vec::new(),
        })
    }

    /// The catalog to browse: the user cache when it parses, else the built-in.
    pub fn load(cache: &Path) -> Self {
        std::fs::read_to_string(cache)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(Self::builtin)
    }

    /// Look a core up by its exact buildbot name.
    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.cores.iter().find(|core| core.name == name)
    }

    /// Case-insensitive substring search over name, display name and system.
    /// An empty query lists everything. Blocked cores are never listed.
    pub fn search(&self, query: &str) -> Vec<&CatalogEntry> {
        let q = query.to_ascii_lowercase();
        self.cores
            .iter()
            .filter(|core| !is_blocked(&core.name))
            .filter(|core| {
                q.is_empty()
                    || core.name.to_ascii_lowercase().contains(&q)
                    || core.display_name.to_ascii_lowercase().contains(&q)
                    || core.system.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }
}

/// The host platform, laid out the way the buildbot directory tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// `apple/osx`, `linux`, `windows`.
    pub os: &'static str,
    /// `arm64`, `x86_64`.
    pub arch: &'static str,
    /// The module extension without the dot (`dylib`, `so`, `dll`).
    pub module_ext: &'static str,
}

impl Platform {
    /// This build's platform, or `None` when the buildbot has no directory for
    /// it (then only the built-in catalog is useful).
    pub fn current() -> Option<Self> {
        let (os, arch) = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            ("apple/osx", "arm64")
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            ("apple/osx", "x86_64")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            ("linux", "x86_64")
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            ("linux", "aarch64")
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            ("windows", "x86_64")
        } else {
            return None;
        };
        let module_ext = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        Some(Self {
            os,
            arch,
            module_ext,
        })
    }

    /// The directory that holds this platform's `latest/` builds.
    pub fn dir(self, base: &str) -> String {
        format!(
            "{}/{}/{}/latest",
            base.trim_end_matches('/'),
            self.os,
            self.arch
        )
    }
}

impl CatalogEntry {
    /// The module file name inside the archive (`<name>_libretro.<ext>`).
    pub fn module_file(&self, platform: Platform) -> String {
        format!("{}_libretro.{}", self.name, platform.module_ext)
    }

    /// The `.zip` URL for this core on `platform`.
    pub fn download_url(&self, base: &str, platform: Platform) -> String {
        format!("{}/{}.zip", platform.dir(base), self.module_file(platform))
    }
}

/// Where the user cache lives (`<app data>/cores/catalog.json`).
pub fn cache_path(cores_dir: &Path) -> PathBuf {
    cores_dir.join("catalog.json")
}

/// Where the downloaded-core registry lives (`<app data>/cores/downloaded.json`).
pub fn registry_path(cores_dir: &Path) -> PathBuf {
    cores_dir.join("downloaded.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_snapshot_parses_and_has_the_expected_shape() {
        let catalog = Catalog::builtin();
        assert!(catalog.cores.len() > 100, "{}", catalog.cores.len());
        let snes = catalog.get("snes9x").expect("snes9x in the snapshot");
        assert_eq!(snes.system, "super_nes");
        assert!(snes.extensions.contains("sfc"));
        let mame = catalog.get("mame").expect("mame in the snapshot");
        assert_eq!(mame.display_name, "Arcade (MAME)");
    }

    #[test]
    fn search_matches_name_display_and_system() {
        let catalog = Catalog::builtin();
        assert!(catalog
            .search("snes9x")
            .iter()
            .any(|core| core.name == "snes9x"));
        assert!(catalog
            .search("super_nes")
            .iter()
            .any(|core| core.name == "snes9x"));
        assert!(!catalog
            .search("no-such-core-xyz")
            .iter()
            .any(|c| c.name == "snes9x"));
        // An empty query lists everything except the blocked cores.
        assert!(catalog
            .search("")
            .iter()
            .all(|core| !is_blocked(&core.name)));
        assert!(catalog.search("").len() < catalog.cores.len());
    }

    #[test]
    fn blocked_cores_are_never_listed() {
        let catalog = Catalog::builtin();
        // The snapshot has it, but the search must not surface it.
        assert!(catalog.get("squirreljme").is_some());
        assert!(is_blocked("SquirrelJME"));
        assert!(!catalog
            .search("squirreljme")
            .iter()
            .any(|c| c.name == "squirreljme"));
        assert!(!catalog.search("j2me").iter().any(|c| is_blocked(&c.name)));
    }

    #[test]
    fn unstable_cores_stay_listed_but_are_flagged() {
        // `is_unstable` drives the yellow warning in the download list; unlike
        // the blocklist, an unstable core is still offered.
        assert!(is_unstable("pcsx2"));
        assert!(is_unstable("PCSX2"));
        assert!(is_unstable("armsx2"));
        assert!(is_unstable("pcee2"));
        assert!(!is_unstable("mesen"));
        assert!(!is_unstable("play"));
        let catalog = Catalog::builtin();
        assert!(catalog.search("pcsx2").iter().any(|c| c.name == "pcsx2"));
    }

    #[test]
    fn the_platform_builds_the_buildbot_url() {
        let platform = Platform {
            os: "apple/osx",
            arch: "arm64",
            module_ext: "dylib",
        };
        let entry = CatalogEntry {
            name: "mame".to_string(),
            display_name: "Arcade (MAME)".to_string(),
            system: "mame".to_string(),
            extensions: "zip".to_string(),
        };
        assert_eq!(entry.module_file(platform), "mame_libretro.dylib");
        assert_eq!(
            entry.download_url("https://buildbot.libretro.com/nightly", platform),
            "https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/mame_libretro.dylib.zip"
        );
        // A trailing slash on the base is harmless.
        assert_eq!(
            entry.download_url("https://example.com/", platform),
            "https://example.com/apple/osx/arm64/latest/mame_libretro.dylib.zip"
        );
    }

    #[test]
    fn a_missing_cache_falls_back_to_the_builtin() {
        let catalog = Catalog::load(Path::new("/no/such/catalog.json"));
        assert!(!catalog.cores.is_empty());
    }
}
