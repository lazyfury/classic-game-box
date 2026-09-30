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
    pub snes_core: Option<String>,
    pub genesis_core: Option<String>,
    pub sms_core: Option<String>,
    pub gg_core: Option<String>,
    pub sg1000_core: Option<String>,
    pub arcade_core: Option<String>,
    pub n64_core: Option<String>,
    pub psp_core: Option<String>,
    pub ps1_core: Option<String>,
    pub j2me_core: Option<String>,
    /// The game library folder: the database, screenshots, saves and cheats
    /// all live under it, so the folder is one self-contained library that can
    /// be copied between machines. `None` means none was chosen yet.
    pub library_root: Option<String>,
    /// Migration input only: the folders listed before the library became a
    /// single folder. The first one is adopted as `library_root` once, then
    /// this is never written again (`skip_serializing`), so a settings file in
    /// the old multi-folder shape keeps its library without keeping the
    /// feature.
    #[serde(rename = "library_dirs", default, skip_serializing)]
    pub legacy_library_dirs: Vec<String>,
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
    /// The geometry anti-aliasing (MSAA) mode key (`"auto"` / `"off"` /
    /// `"2x"` / `"4x"`); an unknown or missing key means auto.
    pub msaa: String,
    /// The UI theme key (`"game"` / `"default"`); an unknown or missing key
    /// means the app picks its own default.
    pub theme: Option<String>,
    /// Whether the light appearance is used (default: dark).
    pub light: bool,
    /// Remembered core options, keyed by `"<core key>:<option key>"`.
    pub core_options: BTreeMap<String, String>,
    /// The middle column's width in logical pixels (0 means the default).
    #[serde(alias = "middle_width")]
    pub content_width: f32,
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
            SystemId::Snes => self.snes_core.as_deref(),
            SystemId::Genesis => self.genesis_core.as_deref(),
            SystemId::MasterSystem => self.sms_core.as_deref(),
            SystemId::GameGear => self.gg_core.as_deref(),
            SystemId::Sg1000 => self.sg1000_core.as_deref(),
            SystemId::Arcade => self.arcade_core.as_deref(),
            SystemId::N64 => self.n64_core.as_deref(),
            SystemId::Psp => self.psp_core.as_deref(),
            SystemId::PlayStation => self.ps1_core.as_deref(),
            SystemId::J2me => self.j2me_core.as_deref(),
        }
    }

    /// Remember a core pick for a console.
    pub fn set_core_key(&mut self, system: SystemId, key: Option<&str>) {
        let slot = match system {
            SystemId::Nes => &mut self.nes_core,
            SystemId::Gba => &mut self.gba_core,
            SystemId::Gb => &mut self.gb_core,
            SystemId::Snes => &mut self.snes_core,
            SystemId::Genesis => &mut self.genesis_core,
            SystemId::MasterSystem => &mut self.sms_core,
            SystemId::GameGear => &mut self.gg_core,
            SystemId::Sg1000 => &mut self.sg1000_core,
            SystemId::Arcade => &mut self.arcade_core,
            SystemId::N64 => &mut self.n64_core,
            SystemId::Psp => &mut self.psp_core,
            SystemId::PlayStation => &mut self.ps1_core,
            SystemId::J2me => &mut self.j2me_core,
        };
        *slot = key.map(str::to_string);
    }

    /// The library folder to open: the remembered one, or the first folder
    /// from the old multi-folder layout, so an install that predates the
    /// switch keeps its library.
    pub fn library_folder(&self) -> Option<&str> {
        self.library_root
            .as_deref()
            .or_else(|| self.legacy_library_dirs.first().map(String::as_str))
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
        assert!(settings.library_root.is_none());
        assert_eq!(settings.legacy_library_dirs, ["/roms"]);
    }

    #[test]
    fn the_old_multi_folder_list_is_read_once_but_never_written() {
        // An install from before the single-library switch keeps its folder…
        let json = r#"{"library_dirs":["/games/First","/games/Second"]}"#;
        let settings: Settings = serde_json::from_str(json).expect("parse");
        assert_eq!(settings.library_folder(), Some("/games/First"));
        // …and the list is gone once the settings are saved again.
        let out = serde_json::to_string(&settings).expect("serialize");
        assert!(!out.contains("library_dirs"));
    }

    #[test]
    fn the_content_width_reads_the_old_middle_width_key() {
        // A settings file written before the field was renamed to
        // `content_width` still loads its dragged split.
        let old: Settings =
            serde_json::from_str("{\"middle_width\":420.0}").expect("old settings parse");
        assert_eq!(old.content_width, 420.0);
    }

    #[test]
    fn the_library_root_round_trips() {
        let settings = Settings {
            library_root: Some("/games/Fc Library".to_string()),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let reloaded: Settings = serde_json::from_str(&json).expect("parse");
        assert_eq!(reloaded.library_root.as_deref(), Some("/games/Fc Library"));
        assert_eq!(reloaded.library_folder(), Some("/games/Fc Library"));
    }

    #[test]
    fn the_msaa_mode_round_trips_and_defaults_to_empty() {
        let settings = Settings {
            msaa: "2x".to_string(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let reloaded: Settings = serde_json::from_str(&json).expect("parse");
        assert_eq!(reloaded.msaa, "2x");

        // A file written before the setting existed has no key at all.
        let old: Settings = serde_json::from_str("{\"shader\":\"crt\"}").expect("parse");
        assert!(old.msaa.is_empty());
    }
}
