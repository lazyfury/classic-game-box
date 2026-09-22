//! Which core runs which console.
//!
//! A core is more than a file name. The frame rate and the sample rate are
//! properties of the console (and, for mGBA, of the machine it is emulating),
//! and the audio pipeline is built around the sample rate — so they travel
//! with the core here rather than being read from the core. mGBA's geometry
//! only answers once a game is loaded, and the audio device has to exist
//! before that, so the numbers are known up front.
//!
//! Values, as reported through `retro_get_system_av_info`:
//!
//! ```text
//!   Mesen    256x240, 60.0998 fps, 48000 Hz
//!   GBA      240x160, 59.7275 fps, 65536 Hz
//!   GB/GBC   160x144, 59.7275 fps, 131072 Hz  (mGBA resamples the GB clock)
//! ```

use std::path::{Path, PathBuf};

use crate::{system_for_path, SystemId};

/// A libretro core.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoreId {
    Mesen,
    Mgba,
}

impl CoreId {
    /// A stable string key for persistence. Case-insensitive on parse.
    pub fn key(self) -> &'static str {
        match self {
            CoreId::Mesen => "mesen",
            CoreId::Mgba => "mgba",
        }
    }

    /// Parse a [`CoreId::key`]; unknown strings are `None`.
    pub fn from_key(key: &str) -> Option<CoreId> {
        match key.to_ascii_lowercase().as_str() {
            "mesen" => Some(CoreId::Mesen),
            "mgba" => Some(CoreId::Mgba),
            _ => None,
        }
    }
}

/// The metadata for every `(core, console)` pair this application can run.
#[derive(Clone, Copy, Debug)]
pub struct CoreChoice {
    /// Which core, for comparing one choice against another.
    pub id: CoreId,
    /// What the settings screen calls it.
    pub name: &'static str,
    /// The console it emulates.
    pub system: SystemId,
    /// The native libretro module file name inside the cores directory.
    pub dylib: &'static str,
    /// What the core produces, in samples per second.
    pub sample_rate: u32,
    /// One emulated frame, in seconds.
    pub frame_seconds: f64,
}

impl CoreChoice {
    /// One emulated frame, in milliseconds.
    pub fn frame_millis(self) -> f64 {
        self.frame_seconds * 1000.0
    }

    /// This registry entry with the module path the app located on disk.
    ///
    /// [`CoreChoice`] is `'static` and carries only a file name; the app owns
    /// the packaged-vs-dev search, so it hands the resolved path back here.
    pub fn with_module(self, module: impl Into<PathBuf>) -> CoreSpec {
        CoreSpec {
            key: self.id.key().to_string(),
            name: self.name.to_string(),
            system: self.system,
            module: module.into(),
            sample_rate: self.sample_rate,
            frame_seconds: self.frame_seconds,
        }
    }
}

impl From<CoreChoice> for CoreSpec {
    fn from(choice: CoreChoice) -> Self {
        choice.with_module(choice.dylib)
    }
}

/// A core resolved to something the app can `dlopen`.
///
/// [`CoreChoice`] is the static, `Copy` registry; this is its owned form, and
/// it is what the session runs. It is also how a **user-supplied** libretro
/// module enters the system (`--core <path>`): the console comes from the ROM
/// extension, and the timing hints are refined from the core's own `av_info`
/// once a game is loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct CoreSpec {
    /// The stable key a pick is stored under: a registry key or a manifest
    /// key for a custom core.
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
}

impl CoreSpec {
    /// A user-supplied libretro module for `system`. The key/name is the file
    /// stem; the timing hints are placeholders the core overrides after load.
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
        }
    }

    /// Whether the module looks like a loadable library by extension. Used by
    /// `--core` to tell a registry key from a path.
    pub fn looks_like_module(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("dylib" | "so" | "dll")
        )
    }
}

/// Every `(core, console)` pair, with the rates the video/audio pipelines are
/// built around. mGBA appears twice: a GBA and a Game Boy are different
/// machines, with different sample rates.
pub const CORES: &[CoreChoice] = &[
    CoreChoice {
        id: CoreId::Mesen,
        name: "Mesen",
        system: SystemId::Nes,
        dylib: "mesen_libretro.dylib",
        sample_rate: 48_000,
        frame_seconds: 1.0 / 60.0998,
    },
    CoreChoice {
        id: CoreId::Mgba,
        name: "mGBA",
        system: SystemId::Gba,
        dylib: "mgba_libretro.dylib",
        sample_rate: 65_536,
        frame_seconds: 1.0 / 59.7275,
    },
    CoreChoice {
        id: CoreId::Mgba,
        name: "mGBA",
        system: SystemId::Gb,
        dylib: "mgba_libretro.dylib",
        sample_rate: 131_072,
        frame_seconds: 1.0 / 59.7275,
    },
];

/// Which cores a console has, in the order the settings screen lists them.
/// **Order is default**: the first entry is what runs without a player choice.
///
/// There is one core per console today (the custom FC core was removed on
/// purpose), but the table is kept as a list so adding a second core later is
/// a data change, not a code change. That was the whole point of the old
/// `CORES_BY_SYSTEM` table, and it still is.
pub const CORES_BY_SYSTEM: &[(SystemId, &[CoreId])] = &[
    (SystemId::Nes, &[CoreId::Mesen]),
    (SystemId::Gba, &[CoreId::Mgba]),
    (SystemId::Gb, &[CoreId::Mgba]),
];

/// The player's core pick per console. `None` means "use the default".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreSelection {
    nes: Option<CoreId>,
    gba: Option<CoreId>,
    gb: Option<CoreId>,
}

impl CoreSelection {
    pub fn get(&self, system: SystemId) -> Option<CoreId> {
        match system {
            SystemId::Nes => self.nes,
            SystemId::Gba => self.gba,
            SystemId::Gb => self.gb,
        }
    }

    pub fn set(&mut self, system: SystemId, core: Option<CoreId>) {
        match system {
            SystemId::Nes => self.nes = core,
            SystemId::Gba => self.gba = core,
            SystemId::Gb => self.gb = core,
        }
    }
}

/// The `(core, console)` entry for this pair, if the registry has one.
fn core(system: SystemId, id: CoreId) -> Option<&'static CoreChoice> {
    CORES.iter().find(|c| c.system == system && c.id == id)
}

/// The cores that can run a console, in listing order. A pair the shared table
/// lists but this file has no entry for is dropped rather than offered and
/// then failed.
pub fn cores_for(system: SystemId) -> Vec<&'static CoreChoice> {
    CORES_BY_SYSTEM
        .iter()
        .find(|(s, _)| *s == system)
        .map(|(_, ids)| ids.iter().filter_map(|id| core(system, *id)).collect())
        .unwrap_or_default()
}

/// The core to run a game on. The player's pick wins; without one, or with an
/// id that does not target the console, the default is used. Never panics:
/// every supported console has at least one core.
pub fn choose_core(path: &str, selection: &CoreSelection) -> &'static CoreChoice {
    let system = system_for_path(path);
    let available = cores_for(system);
    selection
        .get(system)
        .and_then(|wanted| available.iter().copied().find(|c| c.id == wanted))
        .unwrap_or_else(|| {
            *available
                .first()
                .expect("every supported console has at least one core")
        })
}

/// Whether two choices need different machines. Two cores differ when they are
/// different modules, or when the same module renders a different console (mGBA
/// on a GBA versus a Game Boy — different sample rate, and the previous machine
/// cannot become the other).
pub fn same_core(a: Option<&CoreChoice>, b: &CoreChoice) -> bool {
    a.is_some_and(|a| a.id == b.id && a.system == b.system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_console_has_a_core() {
        for system in crate::SYSTEMS {
            assert!(!cores_for(*system).is_empty(), "{system:?} has no core");
        }
    }

    #[test]
    fn a_nes_game_defaults_to_mesen() {
        let choice = choose_core("mario.nes", &CoreSelection::default());
        assert_eq!(choice.id, CoreId::Mesen);
        assert_eq!(choice.system, SystemId::Nes);
    }

    #[test]
    fn a_gba_game_defaults_to_mgba_at_the_gba_rate() {
        let choice = choose_core("pokemon.gba", &CoreSelection::default());
        assert_eq!(choice.id, CoreId::Mgba);
        assert_eq!(choice.sample_rate, 65_536);
    }

    #[test]
    fn a_game_boy_defaults_to_mgba_at_the_gb_rate() {
        let choice = choose_core("tetris.gb", &CoreSelection::default());
        assert_eq!(choice.id, CoreId::Mgba);
        assert_eq!(choice.sample_rate, 131_072);
        // Same module, different machine: same_core is false across systems.
        let gba = choose_core("pokemon.gba", &CoreSelection::default());
        assert!(!same_core(Some(gba), choice));
        assert!(same_core(Some(gba), gba));
    }

    #[test]
    fn a_registry_choice_resolves_to_a_spec_with_the_module() {
        let choice = choose_core("mario.nes", &CoreSelection::default());
        let spec = choice.with_module("/dist/mesen_libretro.dylib");
        assert_eq!(spec.key, "mesen");
        assert_eq!(spec.name, "Mesen");
        assert_eq!(spec.system, SystemId::Nes);
        assert_eq!(spec.module, PathBuf::from("/dist/mesen_libretro.dylib"));
        assert_eq!(spec.sample_rate, 48_000);
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
    }

    #[test]
    fn a_saved_selection_for_the_wrong_console_is_ignored() {
        let mut selection = CoreSelection::default();
        // mGBA targets GBA/GB, never NES; a saved mismatch must not crash.
        selection.set(SystemId::Nes, Some(CoreId::Mgba));
        let choice = choose_core("mario.nes", &selection);
        assert_eq!(choice.system, SystemId::Nes);
        assert_eq!(choice.id, CoreId::Mesen);
    }
}
