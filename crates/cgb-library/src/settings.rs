//! The settings file: a small JSON document next to the library.
//!
//! Which core runs which console, and which folders the library scans. The
//! core picks are stored as strings (see `CoreId::key`) so renaming an enum
//! variant is a code change, not a migration.

use std::path::Path;

use cgb_systems::{CoreId, CoreSelection, SystemId};
use serde::{Deserialize, Serialize};

/// Everything remembered between runs (so far).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub nes_core: Option<String>,
    pub gba_core: Option<String>,
    pub gb_core: Option<String>,
    /// Folders the library scans for ROMs.
    pub library_dirs: Vec<String>,
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

    /// The persisted picks as a [`CoreSelection`], ignoring unknown core
    /// names so a stale file cannot select a core that no longer exists.
    pub fn core_selection(&self) -> CoreSelection {
        let mut selection = CoreSelection::default();
        selection.set(
            SystemId::Nes,
            self.nes_core.as_deref().and_then(CoreId::from_key),
        );
        selection.set(
            SystemId::Gba,
            self.gba_core.as_deref().and_then(CoreId::from_key),
        );
        selection.set(
            SystemId::Gb,
            self.gb_core.as_deref().and_then(CoreId::from_key),
        );
        selection
    }

    /// Remember a core pick for a console.
    pub fn set_core(&mut self, system: SystemId, core: Option<CoreId>) {
        let slot = match system {
            SystemId::Nes => &mut self.nes_core,
            SystemId::Gba => &mut self.gba_core,
            SystemId::Gb => &mut self.gb_core,
        };
        *slot = core.map(|core| core.key().to_string());
    }
}
