//! The settings file: a small JSON document next to the library.
//!
//! Which core runs which console, and which folders the library scans. The
//! core picks are stored as manifest keys (strings), so a core can be renamed
//! or swapped in `cores.json` without a settings migration; a key that no
//! longer exists just falls back to the console default.

use std::collections::BTreeMap;
use std::path::Path;

use cgb_systems::SystemId;
use serde::{Deserialize, Serialize};

/// Everything remembered between runs (so far).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub nes_core: Option<String>,
    pub gba_core: Option<String>,
    pub gb_core: Option<String>,
    pub genesis_core: Option<String>,
    pub sms_core: Option<String>,
    pub gg_core: Option<String>,
    pub sg1000_core: Option<String>,
    pub arcade_core: Option<String>,
    /// Folders the library scans for ROMs.
    pub library_dirs: Vec<String>,
    /// ROM files added on their own (dragged in, or chosen in the file
    /// dialog), outside any scanned folder. Kept as long as the file exists,
    /// so they survive a rescan without being copied into the ROM folder.
    pub added_roms: Vec<String>,
    /// Library sort key (`"name"` / `"size"`). An unknown key falls back to
    /// the name order, so a removed sort mode needs no migration.
    pub library_sort: String,
    /// Whether the library sort runs descending.
    pub library_sort_desc: bool,
    /// The game-picture post-process preset key (`"scanlines"`, `"crt"`, …);
    /// an unknown key means no effect.
    pub shader: String,
    /// Remembered core options, keyed by `"<core key>:<option key>"`.
    pub core_options: BTreeMap<String, String>,
    /// The middle column's width in logical pixels (0 means the default).
    pub middle_width: f32,
}

impl Settings {
    /// Read the file, falling back to defaults if it is missing or invalid.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write the file, creating the parent directory if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, text)
    }

    /// The core key remembered for a console, if any. Validation against the
    /// loaded manifest happens where the cores are chosen, so a stale key just
    /// falls back to the console's default.
    pub fn core_key(&self, system: SystemId) -> Option<&str> {
        match system {
            SystemId::Nes => self.nes_core.as_deref(),
            SystemId::Gba => self.gba_core.as_deref(),
            SystemId::Gb => self.gb_core.as_deref(),
            SystemId::Genesis => self.genesis_core.as_deref(),
            SystemId::MasterSystem => self.sms_core.as_deref(),
            SystemId::GameGear => self.gg_core.as_deref(),
            SystemId::Sg1000 => self.sg1000_core.as_deref(),
            SystemId::Arcade => self.arcade_core.as_deref(),
        }
    }

    /// Remember a core pick for a console.
    pub fn set_core_key(&mut self, system: SystemId, key: Option<&str>) {
        let slot = match system {
            SystemId::Nes => &mut self.nes_core,
            SystemId::Gba => &mut self.gba_core,
            SystemId::Gb => &mut self.gb_core,
            SystemId::Genesis => &mut self.genesis_core,
            SystemId::MasterSystem => &mut self.sms_core,
            SystemId::GameGear => &mut self.gg_core,
            SystemId::Sg1000 => &mut self.sg1000_core,
            SystemId::Arcade => &mut self.arcade_core,
        };
        *slot = key.map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_without_added_roms_still_load() {
        // A settings file written before individually added ROMs existed.
        let json = r#"{"nes_core":"mesen","library_dirs":["/roms"]}"#;
        let settings: Settings = serde_json::from_str(json).expect("old settings parse");
        assert!(settings.added_roms.is_empty());
        assert_eq!(settings.library_dirs, ["/roms"]);
    }
}
