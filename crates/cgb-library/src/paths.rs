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
        Ok(())
    }

    /// The path of a native core inside the cores directory.
    pub fn core_dylib(&self, file_name: &str) -> PathBuf {
        self.cores.join(file_name)
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
