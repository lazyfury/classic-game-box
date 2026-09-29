//! Picking which core runs which console.
//!
//! The cores are **data now**: the app loads them from `cores.json` (see
//! `cores/README.md`) and hands the list to [`choose_core`]. This module holds
//! only the domain shape ([`CoreSpec`]) and the pure pick — the player's saved
//! key wins, otherwise the first core declared for the console.
//!
//! `cgb-systems` stays dependency-free, so it never parses JSON itself: the
//! manifest loader lives in `cgb-library`, the file paths in `cgb-app`.
//!
//! The timing fields (`sample_rate`, `frame_seconds`) are hints from the
//! manifest. The real values come from the core's own `retro_get_system_av_info`
//! once a game is loaded, so a custom core needs no table entry:
//!
//! ```text
//!   Mesen    256x240, 60.0998 fps, 48000 Hz
//!   GBA      240x160, 59.7275 fps, 65536 Hz
//!   GB/GBC   160x144, 59.7275 fps, 131072 Hz  (mGBA resamples the GB clock)
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::SystemId;

/// A core resolved to something the app can `dlopen`.
///
/// Loaded from a manifest (`key` / `name` / `system` / `dylib`), or built ad
/// hoc for `--core <path>`.
#[derive(Clone, Debug, PartialEq)]
pub struct CoreSpec {
    /// The stable key a pick is stored under. Unique per console, not
    /// globally: one module can serve two consoles (mGBA on GBA and GB).
    pub key: String,
    /// What the UI calls it.
    pub name: String,
    /// The console it runs.
    pub system: SystemId,
    /// The module the host `dlopen`s: a file name to search for, or a path.
    pub module: PathBuf,
    /// A hint for the audio device, in Hz. The real rate comes from `av_info`.
    pub sample_rate: u32,
    /// A hint for the frame clock. The real rate comes from `av_info`.
    pub frame_seconds: f64,
    /// Frontend-recommended core-option values, applied for options the player
    /// has not chosen. A core's own default can be a poor fit for a desktop
    /// frontend — FreeJ2ME-Plus tints every frame with a green LCD backlight
    /// until this one is set — so the manifest can pick a saner start. Empty
    /// means "whatever the core defaults to".
    pub option_defaults: BTreeMap<String, String>,
}

impl CoreSpec {
    /// A user-supplied libretro module for `system` (`--core <path>`). The
    /// key/name is the file stem; the timing hints are placeholders the core
    /// overrides after load.
    pub fn custom(module: impl Into<PathBuf>, system: SystemId) -> Self {
        let module = module.into();
        let name = module
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "自定义核心".to_string());
        Self {
            key: name.clone(),
            name,
            system,
            module,
            sample_rate: 0,
            frame_seconds: 1.0 / 60.0,
            option_defaults: BTreeMap::new(),
        }
    }

    /// Whether the module looks like a loadable library by extension. Used by
    /// `--core` to tell a key from a path.
    pub fn looks_like_module(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("dylib" | "so" | "dll")
        )
    }
}

/// Every core that can run `system`, in manifest order.
pub fn cores_for_system(cores: &[CoreSpec], system: SystemId) -> Vec<&CoreSpec> {
    cores.iter().filter(|core| core.system == system).collect()
}

/// The core to run `system`: the player's saved `key` when it names a core for
/// that console, otherwise the first one declared. `None` when the list has no
/// core for the console.
pub fn choose_core<'a>(
    cores: &'a [CoreSpec],
    system: SystemId,
    key: Option<&str>,
) -> Option<&'a CoreSpec> {
    if let Some(key) = key {
        if let Some(choice) = cores
            .iter()
            .find(|core| core.system == system && core.key == key)
        {
            return Some(choice);
        }
    }
    cores.iter().find(|core| core.system == system)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str, system: SystemId, sample_rate: u32) -> CoreSpec {
        CoreSpec {
            key: key.to_string(),
            name: key.to_string(),
            system,
            module: PathBuf::from(format!("{key}_libretro.dylib")),
            sample_rate,
            frame_seconds: 1.0 / 60.0,
            option_defaults: BTreeMap::new(),
        }
    }

    /// The shapes the built-in manifest actually has: Mesen + two mGBA rows.
    fn builtin() -> Vec<CoreSpec> {
        vec![
            spec("mesen", SystemId::Nes, 48_000),
            spec("mgba", SystemId::Gba, 65_536),
            spec("mgba", SystemId::Gb, 131_072),
        ]
    }

    #[test]
    fn the_first_core_for_a_console_is_the_default() {
        let cores = builtin();
        assert_eq!(
            choose_core(&cores, SystemId::Nes, None).unwrap().key,
            "mesen"
        );
        assert_eq!(
            choose_core(&cores, SystemId::Gba, None)
                .unwrap()
                .sample_rate,
            65_536
        );
    }

    #[test]
    fn a_saved_key_picks_that_core() {
        let mut cores = builtin();
        cores.push(spec("nestopia", SystemId::Nes, 48_000));
        assert_eq!(
            choose_core(&cores, SystemId::Nes, Some("nestopia"))
                .unwrap()
                .key,
            "nestopia"
        );
    }

    #[test]
    fn the_same_key_can_target_two_consoles() {
        let cores = builtin();
        assert_eq!(
            choose_core(&cores, SystemId::Gba, Some("mgba"))
                .unwrap()
                .sample_rate,
            65_536
        );
        assert_eq!(
            choose_core(&cores, SystemId::Gb, Some("mgba"))
                .unwrap()
                .sample_rate,
            131_072
        );
    }

    #[test]
    fn a_key_for_the_wrong_console_falls_back_to_the_default() {
        let cores = builtin();
        // mGBA never targets NES; a stale pick must not fail the launch.
        assert_eq!(
            choose_core(&cores, SystemId::Nes, Some("mgba"))
                .unwrap()
                .key,
            "mesen"
        );
    }

    #[test]
    fn no_core_for_a_console_is_none() {
        assert!(choose_core(&[], SystemId::Nes, None).is_none());
    }

    #[test]
    fn cores_for_system_keeps_manifest_order() {
        let mut cores = builtin();
        cores.push(spec("nestopia", SystemId::Nes, 48_000));
        let nes: Vec<&str> = cores_for_system(&cores, SystemId::Nes)
            .iter()
            .map(|core| core.key.as_str())
            .collect();
        assert_eq!(nes, ["mesen", "nestopia"]);
    }

    #[test]
    fn a_custom_spec_is_named_after_its_module() {
        let spec = CoreSpec::custom("/cores/nestopia_libretro.dylib", SystemId::Nes);
        assert_eq!(spec.key, "nestopia_libretro");
        assert_eq!(spec.name, "nestopia_libretro");
        assert_eq!(spec.system, SystemId::Nes);
        assert!(CoreSpec::looks_like_module(&spec.module));
    }

    #[test]
    fn a_key_is_not_mistaken_for_a_module() {
        assert!(!CoreSpec::looks_like_module(Path::new("mesen")));
        assert!(CoreSpec::looks_like_module(Path::new("x.dylib")));
    }
}
