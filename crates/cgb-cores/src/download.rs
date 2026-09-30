//! Fetching cores from the libretro buildbot: refresh the catalog cache and
//! download one module.
//!
//! Everything here is blocking and meant to be called from a background thread
//! (the CLI runs it directly; a future UI runs it off the update loop).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, CatalogEntry, Platform};

/// A hard cap on a single response, so a wrong URL cannot fill the disk. The
/// largest buildbot core is a few hundred MiB.
const MAX_RESPONSE: u64 = 512 * 1024 * 1024;

/// What went wrong talking to the network or unpacking the archive.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("网络请求失败：{0}")]
    Http(String),
    #[error("读写文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("压缩包损坏：{0}")]
    Zip(String),
    #[error("压缩包里没有核心模块（{0}）")]
    MissingModule(String),
    #[error("序列化目录失败：{0}")]
    Json(String),
}

/// Fetch the current platform's core list and merge it with the built-in
/// metadata (display name / system / extensions). The result is ready to be
/// written to the user cache.
pub fn update_catalog(base: &str, platform: Platform) -> Result<Catalog, DownloadError> {
    let url = format!("{}/", platform.dir(base));
    let html = get_string(&url)?;
    let builtin = Catalog::builtin();

    let mut names = parse_listing(&html, platform.module_ext);
    names.sort();
    names.dedup();

    let cores = names
        .into_iter()
        .map(|name| {
            builtin.get(&name).cloned().unwrap_or_else(|| CatalogEntry {
                display_name: name.clone(),
                name,
                system: String::new(),
                extensions: String::new(),
            })
        })
        .collect();

    Ok(Catalog {
        generated: now_epoch(),
        source: base.trim_end_matches('/').to_string(),
        cores,
    })
}

/// Write a catalog to `path` (creating the parent directory).
pub fn write_catalog(path: &Path, catalog: &Catalog) -> Result<(), DownloadError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(catalog).map_err(|e| DownloadError::Json(e.to_string()))?;
    std::fs::write(path, text)?;
    Ok(())
}

/// The downloaded-core registry, sitting next to the catalog in app data. Its
/// schema matches `cores.json`, so [`crate::load_cores`] parses it too.
#[derive(Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    cores: Vec<RegistryEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct RegistryEntry {
    key: String,
    name: String,
    system: String,
    dylib: String,
}

/// Remember a downloaded core so the app's core picker offers it.
///
/// A core for a console this app does not model is left out of the registry
/// (it still loads via `--core <path>`); a re-download replaces the old row.
/// Returns whether the core was registered.
pub fn register_downloaded(
    registry: &Path,
    entry: &CatalogEntry,
    platform: Platform,
) -> Result<bool, DownloadError> {
    if cgb_systems::SystemId::parse_key(&entry.system).is_none() {
        return Ok(false);
    }
    let mut doc: Registry = std::fs::read_to_string(registry)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    doc.cores.retain(|core| core.key != entry.name);
    doc.cores.push(RegistryEntry {
        key: entry.name.clone(),
        name: if entry.display_name.is_empty() {
            entry.name.clone()
        } else {
            entry.display_name.clone()
        },
        system: entry.system.clone(),
        dylib: entry.module_file(platform),
    });
    doc.cores.sort_by(|a, b| a.key.cmp(&b.key));
    if let Some(parent) = registry.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(&doc).map_err(|e| DownloadError::Json(e.to_string()))?;
    std::fs::write(registry, text)?;
    Ok(true)
}

/// Download `entry`'s archive and write the module into `dest_dir`, returning
/// the module path.
///
/// The module is written to a hidden `.part` file first and renamed into place,
/// so an interrupted download never leaves a half-written library the loader
/// would try to `dlopen`.
pub fn download_core(
    entry: &CatalogEntry,
    base: &str,
    platform: Platform,
    dest_dir: &Path,
) -> Result<PathBuf, DownloadError> {
    download_core_with_progress(entry, base, platform, dest_dir, |_, _| {})
}

/// [`download_core`], reporting bytes received as they arrive. `total` is the
/// server's `Content-Length` when it sent one. The callback runs on the calling
/// thread, so a UI passes a closure that forwards over a channel.
pub fn download_core_with_progress<F>(
    entry: &CatalogEntry,
    base: &str,
    platform: Platform,
    dest_dir: &Path,
    mut on_progress: F,
) -> Result<PathBuf, DownloadError>
where
    F: FnMut(u64, Option<u64>),
{
    std::fs::create_dir_all(dest_dir)?;
    let url = entry.download_url(base, platform);
    let bytes = get_bytes_with_progress(&url, &mut on_progress)?;

    let module = entry.module_file(platform);
    let out = dest_dir.join(&module);
    let part = dest_dir.join(format!(".{module}.part"));

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| DownloadError::Zip(e.to_string()))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| DownloadError::Zip(e.to_string()))?;
        let name = file.name().to_string();
        if name == module || name.ends_with(&format!("/{module}")) {
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            std::fs::write(&part, &data)?;
            std::fs::rename(&part, &out)?;
            return Ok(out);
        }
    }
    Err(DownloadError::MissingModule(module))
}

/// Parse a buildbot h5ai listing for `*_libretro.<ext>.zip` names.
fn parse_listing(html: &str, module_ext: &str) -> Vec<String> {
    let needle = format!("_libretro.{module_ext}.zip");
    let mut names = Vec::new();
    for chunk in html.split("href=\"") {
        let Some(end) = chunk.find('"') else { continue };
        let href = &chunk[..end];
        let Some(stem) = href.strip_suffix(&needle) else {
            continue;
        };
        let name = stem.rsplit('/').next().unwrap_or(stem);
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            names.push(name.to_string());
        }
    }
    names
}

fn now_epoch() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// An agent that honors `https_proxy`/`all_proxy` from the environment (so a
/// machine behind a proxy still works) and gives up after two minutes.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(std::time::Duration::from_secs(120))
        .build()
}

fn get_bytes(url: &str) -> Result<Vec<u8>, DownloadError> {
    get_bytes_with_progress(url, &mut |_, _| {})
}

fn get_bytes_with_progress<F>(url: &str, on_progress: &mut F) -> Result<Vec<u8>, DownloadError>
where
    F: FnMut(u64, Option<u64>),
{
    let response = agent()
        .get(url)
        .call()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > MAX_RESPONSE {
            return Err(DownloadError::Http("响应超过上限".to_string()));
        }
        bytes.extend_from_slice(&buffer[..read]);
        on_progress(received, total);
    }
    Ok(bytes)
}

fn get_string(url: &str) -> Result<String, DownloadError> {
    Ok(String::from_utf8_lossy(&get_bytes(url)?).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_parsed_into_core_names() {
        let html = r#"
            <a href="../">..</a>
            <a href="mame_libretro.dylib.zip">mame_libretro.dylib.zip</a>
            <a href="/nightly/apple/osx/arm64/latest/snes9x_libretro.dylib.zip">snes9x</a>
            <a href="mame2003_plus_libretro.dylib.zip">mame2003_plus</a>
            <a href="something_else.zip">ignore</a>
        "#;
        let names = parse_listing(html, "dylib");
        assert_eq!(names, ["mame", "snes9x", "mame2003_plus"]);
    }

    #[test]
    fn the_listing_parser_ignores_the_wrong_platform_extension() {
        let html = r#"<a href="mame_libretro.so.zip">mame</a>"#;
        assert!(parse_listing(html, "dylib").is_empty());
        assert_eq!(parse_listing(html, "so"), ["mame"]);
    }

    #[test]
    fn registering_a_downloaded_core_writes_a_manifest_row() {
        let dir = std::env::temp_dir().join(format!("cgb-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("downloaded.json");
        let platform = Platform {
            os: "apple/osx",
            arch: "arm64",
            module_ext: "dylib",
        };
        let entry = CatalogEntry {
            name: "bsnes".to_string(),
            display_name: "Nintendo - SNES / SFC (bsnes)".to_string(),
            system: "super_nes".to_string(),
            extensions: "sfc".to_string(),
        };
        // Registering twice replaces, it does not duplicate.
        assert!(register_downloaded(&path, &entry, platform).unwrap());
        assert!(register_downloaded(&path, &entry, platform).unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let cores = doc["cores"].as_array().unwrap();
        assert_eq!(cores.len(), 1);
        assert_eq!(cores[0]["system"], "super_nes");
        assert_eq!(cores[0]["dylib"], "bsnes_libretro.dylib");

        // A console this app does not model is left out of the registry.
        let wiiu = CatalogEntry {
            name: "cemu".to_string(),
            display_name: "Cemu".to_string(),
            system: "wiiu".to_string(),
            extensions: "wud".to_string(),
        };
        assert!(!register_downloaded(&path, &wiiu, platform).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_catalog_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("cgb-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("catalog.json");

        let catalog = Catalog {
            generated: "123".to_string(),
            source: "https://example.com".to_string(),
            cores: vec![CatalogEntry {
                name: "mame".to_string(),
                display_name: "Arcade (MAME)".to_string(),
                system: "mame".to_string(),
                extensions: "zip".to_string(),
            }],
        };
        write_catalog(&path, &catalog).unwrap();
        let reloaded = Catalog::load(&path);
        assert_eq!(reloaded.cores.len(), 1);
        assert_eq!(reloaded.cores[0].name, "mame");
        assert_eq!(reloaded.source, "https://example.com");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
