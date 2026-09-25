//! Where the app keeps its files.
//!
//! Two roots, because two kinds of thing live different lives:
//!
//! - the **game library** (`root`) is one self-contained folder. The database,
//!   screenshots, save states / battery saves and cheats all live under it, so
//!   copying that folder to another machine brings the whole library with it.
//! - **app data** (`user_data`) holds what belongs to the install rather than
//!   the games: the settings pointer that names the library, the shipped
//!   cores, and the seeded BIOS (`system`) the cores probe. The BIOS directory
//!   stays out of the library because a `.zip` in it (`neogeo.zip`) would be
//!   mistaken for an arcade ROM by the folder scan.
//!
//! Until a library has been chosen, the game data keeps the old layout under
//! `user_data` (so an existing install keeps working and keeps scanning its
//! built-in `roms` folder). Choosing a library folder moves the game data
//! under it, from then on.
//!
//! A `--selfcheck` run points both roots at one temp directory through
//! [`Paths::under`], so it leaves nothing behind.

use std::path::{Path, PathBuf};

/// The directories the app reads and writes.
#[derive(Clone, Debug)]
pub struct Paths {
    /// The game-data root: the chosen library, or `user_data` until one is
    /// chosen. Everything the app makes about a game lives under it.
    pub root: PathBuf,
    /// The chosen game library folder, or `None` while the app is still using
    /// the layout under `user_data` (no library chosen yet).
    pub library_root: Option<PathBuf>,
    /// App data: settings, shipped cores and the seeded BIOS. Not the library.
    pub user_data: PathBuf,
    /// BIOS/system files the cores may probe (`disksys.rom`, `gba_bios.bin`).
    pub system: PathBuf,
    /// Save states and `.srm` battery saves.
    pub saves: PathBuf,
    /// Screenshot PNGs (a sibling of the database, inside the library).
    pub screenshots: PathBuf,
    /// Cheat files (`.cht`), one per game.
    pub cheats: PathBuf,
    /// A built-in ROM folder that is always scanned (handy until the UI has an
    /// "add folder" control). Default: `<root>/roms`.
    pub roms: PathBuf,
    /// The SQLite game library.
    pub library_db: PathBuf,
    /// The settings JSON, in app data: the library cannot be found without it.
    pub settings_json: PathBuf,
    /// Native libretro cores shipped with the app.
    pub cores: PathBuf,
}

impl Paths {
    /// The default layout: app data in the platform data directory, and the
    /// game library wherever the settings say it is.
    ///
    /// The library folder is chosen, not derived from the platform directory:
    /// it comes from `library_root` (or, on the first run after the switch, the
    /// first folder of the old multi-folder layout). Until something is chosen
    /// the game data stays in app data. Once chosen it is remembered in
    /// `settings.json` — which is why settings lives in app data: the library
    /// cannot be opened before it is located.
    pub fn platform() -> Self {
        let user_data = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Classic Game Box");
        let settings = crate::settings::Settings::load(&user_data.join("settings.json"));
        let library_root = settings.library_folder().map(PathBuf::from);
        Self::new(user_data, library_root)
    }

    /// The layout rooted at one explicit directory (used by tests and
    /// `--selfcheck`): settings, cores and the game data all share it.
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self::new(root, None)
    }

    /// The layout for a chosen game library, with app data at `user_data`.
    ///
    /// `library_root` is `Some` once a library has been chosen (game data
    /// lives under it) and `None` while it has not (game data lives under
    /// `user_data`).
    pub fn new(user_data: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        let user_data = user_data.into();
        let root = library_root.clone().unwrap_or_else(|| user_data.clone());
        Self {
            system: user_data.join("system"),
            saves: root.join("saves"),
            screenshots: root.join("screenshots"),
            cheats: root.join("cheats"),
            roms: root.join("roms"),
            library_db: root.join("library.db"),
            settings_json: user_data.join("settings.json"),
            cores: user_data.join("cores"),
            library_root,
            user_data,
            root,
        }
    }

    /// Whether a game library folder has been chosen.
    pub fn has_library(&self) -> bool {
        self.library_root.is_some()
    }

    /// Create the directories that must exist before use.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.user_data)?;
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.system)?;
        std::fs::create_dir_all(&self.saves)?;
        std::fs::create_dir_all(&self.screenshots)?;
        std::fs::create_dir_all(&self.cheats)?;
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
///
/// The core key is part of the name because libretro save states are core
/// private: a state a Mesen made cannot be read by FBNeo, so the two must not
/// collide on one slot.
pub fn save_state_path(saves: &Path, rom: &Path, core_key: &str, slot: u8) -> PathBuf {
    let name = rom
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    saves.join(format!("{name}.{core_key}.state{slot}"))
}

/// A slot's thumbnail (a PNG beside the state), or `None` for the quick slot
/// if it has none.
pub fn save_state_thumb_path(saves: &Path, rom: &Path, core_key: &str, slot: u8) -> PathBuf {
    let name = rom
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    saves.join(format!("{name}.{core_key}.state{slot}.png"))
}

/// A ROM's battery-save (`RETRO_MEMORY_SAVE_RAM`) path.
pub fn battery_save_path(saves: &Path, rom: &Path) -> PathBuf {
    let name = rom
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    saves.join(format!("{name}.srm"))
}

/// A game's cheat file inside the cheats directory.
pub fn cheat_file(cheats: &Path, rom: &Path) -> PathBuf {
    let stem = rom
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    cheats.join(format!("{stem}.cht"))
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
        let path = save_state_path(
            Path::new("/saves"),
            Path::new("/roms/mario.nes"),
            "mesen",
            2,
        );
        assert_eq!(path, PathBuf::from("/saves/mario.nes.mesen.state2"));
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

    #[test]
    fn game_data_lives_in_the_library_and_app_data_does_not() {
        let paths = Paths::new("/app data", Some(PathBuf::from("/games/Fc Library")));
        // The library is self-contained: the database and everything the app
        // makes about a game live under it, so copying the folder is enough.
        for path in [
            &paths.library_db,
            &paths.screenshots,
            &paths.saves,
            &paths.cheats,
            &paths.roms,
        ] {
            assert!(
                path.starts_with("/games/Fc Library"),
                "{} escaped the library",
                path.display()
            );
        }
        // What belongs to the install stays in app data.
        for path in [&paths.settings_json, &paths.cores, &paths.system] {
            assert!(
                path.starts_with("/app data"),
                "{} landed in the library",
                path.display()
            );
        }
    }

    #[test]
    fn without_a_chosen_library_the_game_data_stays_in_app_data() {
        // An install that never picked a folder keeps the old layout, so its
        // built-in `roms` folder and database are not abandoned.
        let paths = Paths::new("/app data", None);
        assert!(!paths.has_library());
        assert_eq!(paths.root, paths.user_data);
        assert_eq!(paths.library_db, PathBuf::from("/app data/library.db"));
        assert_eq!(paths.roms, PathBuf::from("/app data/roms"));
    }

    #[test]
    fn under_keeps_app_data_and_library_together() {
        // `--selfcheck` and tests need one folder that holds everything.
        let paths = Paths::under("/tmp/selfcheck");
        assert_eq!(paths.root, paths.user_data);
        assert!(paths.library_db.starts_with("/tmp/selfcheck"));
        assert!(paths.cores.starts_with("/tmp/selfcheck"));
    }
}
