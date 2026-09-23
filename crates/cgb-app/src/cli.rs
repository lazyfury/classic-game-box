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

use cgb_systems::CoreSpec;

/// Everything the process was asked to do at startup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Load this game once the window is up.
    pub rom: Option<PathBuf>,
    /// Force a core instead of the console's default.
    pub core: Option<CoreOverride>,
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
    classic-game-box [--rom <path>] [--core <key|module>]

OPTIONS:
    --rom <path>     Load a ROM at startup (also accepted positionally).
    --core <value>   Force a core: a manifest key (mesen, mgba, nestopia, …)
                     or a path to a .dylib / .so / .dll.
    -h, --help       Print this help.

EXAMPLES:
    classic-game-box mario.nes
    classic-game-box --rom mario.nes --core mesen
    classic-game-box --rom mario.nes --core ./nestopia_libretro.dylib";

impl Args {
    /// Parse arguments **without** the program name.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut out = Self::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--rom" => out.rom = Some(PathBuf::from(require_value("--rom", &mut iter)?)),
                "--core" => out.core = Some(parse_core(&require_value("--core", &mut iter)?)?),
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
