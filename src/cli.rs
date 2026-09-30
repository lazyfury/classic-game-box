//! Command-line arguments.
//!
//! ```text
//! classic-game-box [--rom <path>] [--core <key|module>]
//! ```
//!
//! The positional form is kept (`cargo run -p cgb-app -- mario.nes`). `--core`
//! takes either a registered key (`mesen`, `mgba`) or a path to any libretro
//! module — the latter is how a custom core is tried without rebuilding the
//! app or editing the registry.

use std::path::PathBuf;

use crate::ui::ThemeChoice;
use cgb_systems::CoreSpec;

/// Everything the process was asked to do at startup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Load this game once the window is up.
    pub rom: Option<PathBuf>,
    /// Force a core instead of the console's default.
    pub core: Option<CoreOverride>,
    /// Point the app at a game library folder (`--library-dir`). The database,
    /// screenshots, saves and cheats then live under it, so a copied folder is
    /// the whole library. Omitted, the remembered library (or the first
    /// scanned folder) is used.
    pub library_dir: Option<PathBuf>,
    /// Which theme to paint with (`--theme`): the custom house style or the
    /// library default. `None` keeps the saved preference.
    pub theme: Option<ThemeChoice>,
    /// Use the light appearance instead of the dark one (`--light`).
    pub light: bool,
    /// Run the headless checks and exit, without opening a window.
    pub selfcheck: bool,
    /// Refresh the downloadable-core catalog from the network and exit
    /// (`--force-update`).
    pub force_update: bool,
    /// Search the (cached) core catalog and exit (`--search-core <query>`).
    pub search_cores: Option<String>,
    /// Download one core into the app-data cores directory and exit
    /// (`--download-core <name>`).
    pub download_core: Option<String>,
    /// Override the download source base URL (`--core-base-url <url>`).
    pub core_base_url: Option<String>,
}

impl Args {
    /// Whether a core-management flag was given, so `main` runs the headless
    /// core path instead of opening the UI.
    pub fn is_core_command(&self) -> bool {
        self.force_update || self.search_cores.is_some() || self.download_core.is_some()
    }
}

/// Which core `--core` named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreOverride {
    /// A core key from the manifest (`mesen`, `mgba`, `nestopia`, …). Not
    /// validated here — the manifest is only read once the app starts.
    Key(String),
    /// A path to a libretro module on disk.
    Module(PathBuf),
}

/// Printed for `--help` and on a parse error.
pub const USAGE: &str = "\
Classic Game Box

USAGE:
    classic-game-box [--rom <path>] [--core <key|module>] [--library-dir <path>]
                     [--theme <name>] [--light] [--selfcheck]
                     [--force-update] [--search-core <q>] [--download-core <name>]

OPTIONS:
    --rom <path>          Load a ROM at startup (also accepted positionally).
    --core <value>        Force a core: a manifest key (mesen, mgba, nestopia, …)
                          or a path to a .dylib / .so / .dll.
    --library-dir <path>  Use this game library folder: the database,
                          screenshots, saves and cheats live under it. The
                          pick is remembered for later runs.
    --theme <name>        Theme: game (custom house style, default) or default
                          (the library's built-in palette).
    --light               Use the light appearance (default: dark).
    --selfcheck           Run the headless checks (paths, library, settings,
                          cores, icons) and exit; opens no window.
    --force-update        Refresh the downloadable-core catalog (the 下载源) from
                          the libretro buildbot into the app-data cache and exit.
    --search-core <q>     Search the cached core catalog and exit.
    --download-core <name>
                          Download a core by name into the app-data cores
                          directory, then open it to prove it loads, and exit.
    --core-base-url <url> Override the download source (default: the libretro
                          nightly buildbot).
    -h, --help            Print this help.

EXAMPLES:
    classic-game-box mario.nes
    classic-game-box --rom mario.nes --core mesen
    classic-game-box --rom mario.nes --core ./nestopia_libretro.dylib
    classic-game-box --library-dir ~/Documents/FcGameLibrary";

impl Args {
    /// Parse arguments **without** the program name.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut out = Self::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--rom" => out.rom = Some(PathBuf::from(require_value("--rom", &mut iter)?)),
                "--core" => out.core = Some(parse_core(&require_value("--core", &mut iter)?)?),
                "--library-dir" | "--rom-dir" => {
                    out.library_dir =
                        Some(PathBuf::from(require_value("--library-dir", &mut iter)?))
                }
                "--theme" => {
                    let value = require_value("--theme", &mut iter)?;
                    out.theme =
                        Some(ThemeChoice::parse(&value).ok_or_else(|| {
                            format!("未知主题 `{value}`（可用：default | game）")
                        })?);
                }
                "--light" => out.light = true,
                "--selfcheck" => out.selfcheck = true,
                "--force-update" => out.force_update = true,
                "--search-core" => {
                    out.search_cores = Some(require_value("--search-core", &mut iter)?)
                }
                "--download-core" => {
                    out.download_core = Some(require_value("--download-core", &mut iter)?)
                }
                "--core-base-url" => {
                    out.core_base_url = Some(require_value("--core-base-url", &mut iter)?)
                }
                other if other.starts_with('-') => return Err(format!("未知参数：{other}")),
                other if out.rom.is_none() => out.rom = Some(PathBuf::from(other)),
                other => return Err(format!("多余的位置参数：{other}")),
            }
        }
        Ok(out)
    }
}

/// A value for a flag that requires one.
fn require_value(flag: &str, iter: &mut impl Iterator<Item = String>) -> Result<String, String> {
    iter.next().ok_or_else(|| format!("{flag} 需要一个值"))
}

/// A path-looking value is a module; anything else is an unvalidated key (a
/// registry key or a manifest key, checked once the manifest is read).
fn parse_core(value: &str) -> Result<CoreOverride, String> {
    let path = PathBuf::from(value);
    if CoreSpec::looks_like_module(&path) || path.is_file() {
        Ok(CoreOverride::Module(path))
    } else {
        Ok(CoreOverride::Key(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn a_bare_path_is_the_rom() {
        assert_eq!(
            parse(&["mario.nes"]).unwrap().rom,
            Some(PathBuf::from("mario.nes"))
        );
    }

    #[test]
    fn rom_and_core_flags_parse() {
        let parsed = parse(&["--rom", "mario.nes", "--core", "mesen"]).unwrap();
        assert_eq!(parsed.rom, Some(PathBuf::from("mario.nes")));
        assert_eq!(parsed.core, Some(CoreOverride::Key("mesen".to_string())));
    }

    #[test]
    fn a_module_path_is_a_custom_core() {
        let parsed = parse(&["--core", "./nestopia_libretro.dylib"]).unwrap();
        assert_eq!(
            parsed.core,
            Some(CoreOverride::Module(PathBuf::from(
                "./nestopia_libretro.dylib"
            )))
        );
    }

    #[test]
    fn an_unknown_flag_is_an_error() {
        assert!(parse(&["--nope", "x"]).is_err());
    }

    #[test]
    fn selfcheck_parses() {
        assert!(parse(&["--selfcheck"]).unwrap().selfcheck);
    }

    #[test]
    fn core_management_flags_parse() {
        let update = parse(&["--force-update"]).unwrap();
        assert!(update.force_update);
        assert!(update.is_core_command());
        assert_eq!(
            parse(&["--search-core", "snes"])
                .unwrap()
                .search_cores
                .as_deref(),
            Some("snes")
        );
        assert_eq!(
            parse(&["--download-core", "mame"])
                .unwrap()
                .download_core
                .as_deref(),
            Some("mame")
        );
        assert_eq!(
            parse(&["--core-base-url", "https://example.com/x"])
                .unwrap()
                .core_base_url
                .as_deref(),
            Some("https://example.com/x")
        );
        assert!(!Args::default().is_core_command());
        assert!(parse(&["--search-core"]).is_err());
    }

    #[test]
    fn theme_and_appearance_flags_parse() {
        assert_eq!(
            parse(&["--theme", "game"]).unwrap().theme,
            Some(ThemeChoice::Game)
        );
        assert_eq!(
            parse(&["--theme", "default"]).unwrap().theme,
            Some(ThemeChoice::Default)
        );
        assert_eq!(Args::default().theme, None);
        assert!(!Args::default().light);
        assert!(parse(&["--light"]).unwrap().light);
        assert!(parse(&["--theme", "nope"]).is_err());
        assert!(parse(&["--theme"]).is_err());
    }

    #[test]
    fn library_dir_parses_and_its_old_name_is_kept() {
        let path = PathBuf::from("/games/Fc Library");
        assert_eq!(
            parse(&["--library-dir", "/games/Fc Library"])
                .unwrap()
                .library_dir,
            Some(path.clone())
        );
        assert_eq!(
            parse(&["--rom-dir", "/games/Fc Library"])
                .unwrap()
                .library_dir,
            Some(path)
        );
    }

    #[test]
    fn a_missing_flag_value_is_an_error() {
        assert!(parse(&["--core"]).is_err());
        assert!(parse(&["--rom"]).is_err());
    }

    #[test]
    fn a_bare_word_is_an_unvalidated_key() {
        // A custom key cannot be checked until the manifest is read.
        assert_eq!(
            parse(&["--core", "nestopia"]).unwrap().core,
            Some(CoreOverride::Key("nestopia".to_string()))
        );
    }
}
