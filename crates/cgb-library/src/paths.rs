//! Where the app keeps its files: the game library database, settings, save
//! states and battery saves, and the core/system directories handed to
//! libretro through `GET_SYSTEM_DIRECTORY` / `GET_SAVE_DIRECTORY`.
//!
//! All under one root so a `--selfcheck` run can point at a temp directory and
//! leave nothing behind. On macOS the root is
//! `~/Library/Application Support/Classic Game Box`.

use std::path::{Path, PathBuf};

/// The directories the app reads and writes.
#[derive(Clone, Debug)]
pub struct Paths {
    /// The app data root.
    pub root: PathBuf,
    /// BIOS/system files the cores may probe (`disksys.rom`, `gba_bios.bin`).
    pub system: PathBuf,
    /// Save states and `.srm` battery saves.
    pub saves: PathBuf,
    /// Screenshot PNGs (one directory per library; a sibling of the database).
    pub screenshots: PathBuf,
    /// A built-in ROM folder that is always scanned (handy until the UI has an
    /// "add folder" control). Default: `<root>/roms`.
    pub roms: PathBuf,
    /// The SQLite game library.
    pub library_db: PathBuf,
    /// The settings JSON.
    pub settings_json: PathBuf,
    /// Native libretro cores shipped with the app.
    pub cores: PathBuf,
}

impl Paths {
    /// The default layout, under the platform data directory.
    pub fn platform() -> Self {
        let root = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Classic Game Box");
        Self::under(root)
    }

    /// The layout rooted at an explicit directory (used by tests and
    /// `--selfcheck`).
    pub fn under(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            system: root.join("system"),
            saves: root.join("saves"),
            screenshots: root.join("screenshots"),
            roms: root.join("roms"),
            library_db: root.join("library.db"),
            settings_json: root.join("settings.json"),
            cores: root.join("cores"),
            root,
        }
    }

    /// Create the directories that must exist before use.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.system)?;
        std::fs::create_dir_all(&self.saves)?;
        std::fs::create_dir_all(&self.screenshots)?;
        std::fs::create_dir_all(&self.roms)?;
        // Holds the shipped cores and, beside them, `cores.json`.
        std::fs::create_dir_all(&self.cores)?;
        Ok(())
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::platform()
    }
}

/// A ROM's save-state path for a slot (1..=4), or the quick slot (`0`).
pub fn save_state_path(saves: &Path, rom: &Path, slot: u8) -> PathBuf {
    let name = rom
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    saves.join(format!("{name}.state{slot}"))
}

/// A ROM's battery-save (`RETRO_MEMORY_SAVE_RAM`) path.
pub fn battery_save_path(saves: &Path, rom: &Path) -> PathBuf {
    let name = rom
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    saves.join(format!("{name}.srm"))
}

/// Copy files from a bundled, read-only directory into a writable one,
/// skipping names that already exist. Returns how many files were copied.
///
/// The checkout ships the BIOS an arcade core needs under
/// `assets/roms/<system>/system`, but the core is pointed at the writable
/// `<app data>/system` directory through `GET_SYSTEM_DIRECTORY`. Seeding keeps
/// the two apart: the repository stays clean, and a file the player drops into
/// the writable directory is never overwritten. A missing source is not an
/// error (the assets may not be shipped), it just seeds nothing.
pub fn seed_dir(bundled: &Path, target: &Path) -> std::io::Result<usize> {
    if !bundled.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(target)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(bundled)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let dest = target.join(name);
        if dest.exists() {
            continue;
        }
        std::fs::copy(&path, &dest)?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_path_keeps_the_full_name_and_slot() {
        let path = save_state_path(Path::new("/saves"), Path::new("/roms/mario.nes"), 2);
        assert_eq!(path, PathBuf::from("/saves/mario.nes.state2"));
    }

    #[test]
    fn a_battery_path_drops_the_extension() {
        let path = battery_save_path(Path::new("/saves"), Path::new("/roms/mario.nes"));
        assert_eq!(path, PathBuf::from("/saves/mario.srm"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cgb-seed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn seed_copies_missing_files_only() {
        let root = temp_dir("copy");
        let bundled = root.join("bundled");
        let target = root.join("target");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(bundled.join("neogeo.zip"), b"bundled").unwrap();
        std::fs::write(bundled.join("pgm.zip"), b"bundled").unwrap();
        // A player-supplied file with the same name must win.
        std::fs::write(target.join("neogeo.zip"), b"player").unwrap();

        assert_eq!(seed_dir(&bundled, &target).unwrap(), 1);
        assert_eq!(std::fs::read(target.join("neogeo.zip")).unwrap(), b"player");
        assert_eq!(std::fs::read(target.join("pgm.zip")).unwrap(), b"bundled");
        // Seeding again copies nothing.
        assert_eq!(seed_dir(&bundled, &target).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn seed_without_a_source_is_not_an_error() {
        let root = temp_dir("missing");
        let target = root.join("target");
        assert_eq!(seed_dir(&root.join("nope"), &target).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
