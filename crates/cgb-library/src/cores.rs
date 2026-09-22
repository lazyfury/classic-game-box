//! Core manifests: which libretro modules exist, as data.
//!
//! The built-in cores (Mesen, mGBA) and any third-party cores you add all come
//! from `cores.json` files — one loader, no Rust change to add or move a core.
//! See `cores/README.md`.
//!
//! ```json
//! {
//!   "cores": [
//!     {
//!       "key": "nestopia",
//!       "name": "Nestopia",
//!       "system": "nes",
//!       "dylib": "nestopia_libretro.dylib",
//!       "sample_rate": 48000,
//!       "fps": 60.098
//!     }
//!   ]
//! }
//! ```
//!
//! `name` defaults to `key`; `sample_rate` and `fps` are hints only — the real
//! values come from the core's own `av_info` after a game loads. The `dylib`
//! is a file name the app searches for (packaged `cores/`, then `cores/dist/`)
//! or a path. `key` must be unique **per console**: one module can serve two
//! consoles (mGBA appears once for GBA and once for GB).

use std::path::{Path, PathBuf};

use cgb_systems::{CoreSpec, SystemId};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    cores: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    key: String,
    name: Option<String>,
    system: String,
    dylib: String,
    #[serde(default)]
    sample_rate: u32,
    #[serde(default)]
    fps: f64,
}

/// Read a manifest into specs, or `[]` when it is missing. A malformed file is
/// reported and treated as empty rather than crashing startup. Entries are
/// deduped by `(system, key)`, first wins.
pub fn load_cores(path: &Path) -> Vec<CoreSpec> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("cgb: 核心清单 {} 解析失败：{error}", path.display());
            return Vec::new();
        }
    };

    let mut out: Vec<CoreSpec> = Vec::new();
    for entry in manifest.cores {
        let Some(system) = system_of(&entry.system) else {
            eprintln!(
                "cgb: 清单 {} 的核心 {} 机种未知：{}",
                path.display(),
                entry.key,
                entry.system
            );
            continue;
        };
        if out
            .iter()
            .any(|core| core.system == system && core.key == entry.key)
        {
            eprintln!(
                "cgb: 清单 {} 有重复的 key：{} ({})",
                path.display(),
                entry.key,
                system.short()
            );
            continue;
        }
        let fps = if entry.fps > 0.0 { entry.fps } else { 60.0 };
        let name = entry.name.unwrap_or_else(|| entry.key.clone());
        out.push(CoreSpec {
            key: entry.key,
            name,
            system,
            module: PathBuf::from(entry.dylib),
            sample_rate: entry.sample_rate,
            frame_seconds: 1.0 / fps,
        });
    }
    out
}

/// A manifest `system` string. Unknown values are skipped (with a warning)
/// rather than silently defaulting to NES the way [`SystemId::from_key`] does.
fn system_of(key: &str) -> Option<SystemId> {
    match key.to_ascii_lowercase().as_str() {
        "nes" => Some(SystemId::Nes),
        "gba" => Some(SystemId::Gba),
        "gb" | "gbc" => Some(SystemId::Gb),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn write_manifest(text: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cgb-manifest-{}-{n}.json", std::process::id()));
        std::fs::write(&path, text).expect("write manifest");
        path
    }

    #[test]
    fn a_missing_manifest_is_empty() {
        assert!(load_cores(Path::new("/no/such/cores.json")).is_empty());
    }

    #[test]
    fn an_entry_becomes_a_spec() {
        let path = write_manifest(
            r#"{ "cores": [
                { "key": "nestopia", "name": "Nestopia", "system": "nes",
                  "dylib": "nestopia_libretro.dylib", "sample_rate": 48000, "fps": 60.098 }
            ] }"#,
        );
        let cores = load_cores(&path);
        assert_eq!(cores.len(), 1);
        let spec = &cores[0];
        assert_eq!(spec.key, "nestopia");
        assert_eq!(spec.name, "Nestopia");
        assert_eq!(spec.system, SystemId::Nes);
        assert_eq!(spec.module, PathBuf::from("nestopia_libretro.dylib"));
        assert_eq!(spec.sample_rate, 48_000);
        assert!((spec.frame_seconds - 1.0 / 60.098).abs() < 1e-9);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn optional_fields_default_and_unknown_systems_are_skipped() {
        let path = write_manifest(
            r#"{ "cores": [
                { "key": "mystery", "name": "Mystery", "system": "wonderswan",
                  "dylib": "mystery.dylib" },
                { "key": "bare", "system": "gb", "dylib": "bare.dylib" }
            ] }"#,
        );
        let cores = load_cores(&path);
        assert_eq!(cores.len(), 1);
        assert_eq!(cores[0].key, "bare");
        assert_eq!(cores[0].name, "bare");
        assert_eq!(cores[0].system, SystemId::Gb);
        assert!((cores[0].frame_seconds - 1.0 / 60.0).abs() < 1e-9);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn one_key_may_serve_two_consoles() {
        let path = write_manifest(
            r#"{ "cores": [
                { "key": "mgba", "system": "gba", "dylib": "mgba_libretro.dylib" },
                { "key": "mgba", "system": "gb", "dylib": "mgba_libretro.dylib" }
            ] }"#,
        );
        let cores = load_cores(&path);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].system, SystemId::Gba);
        assert_eq!(cores[1].system, SystemId::Gb);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_duplicate_key_within_one_console_is_dropped() {
        let path = write_manifest(
            r#"{ "cores": [
                { "key": "dup", "system": "nes", "dylib": "a.dylib" },
                { "key": "dup", "system": "nes", "dylib": "b.dylib" }
            ] }"#,
        );
        let cores = load_cores(&path);
        assert_eq!(cores.len(), 1);
        assert_eq!(cores[0].module, PathBuf::from("a.dylib"));
        let _ = std::fs::remove_file(&path);
    }
}
