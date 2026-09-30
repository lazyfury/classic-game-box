//! The consoles this application knows and the file extensions that select
//! them. Anything unrecognised is a NES game, which keeps the old behaviour
//! for a file with no extension and gives a clear failure for a file that is
//! not a ROM at all.

use std::path::Path;

/// A console. One console can have more than one core (see
/// [`crate::CoreSpec`]), which is exactly why selection is per console and not
/// per file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SystemId {
    Nes,
    Gba,
    Gb,
    /// Super Nintendo / Super Famicom (the 16-bit Nintendo console). A
    /// software-rendered core; no BIOS or DSP ROM is required.
    Snes,
    /// Sega Mega Drive / Genesis (the 16-bit console).
    Genesis,
    /// Sega Master System.
    MasterSystem,
    /// Sega Game Gear.
    GameGear,
    /// Sega SG-1000.
    Sg1000,
    /// Arcade: a MAME-family core, one ROM set per game.
    Arcade,
    /// Nintendo 64. Hardware-rendered (OpenGL) via Mupen64Plus-Next/GLideN64.
    N64,
    /// PlayStation Portable. Hardware-rendered (OpenGL) via PPSSPP.
    Psp,
    /// Sony PlayStation. Hardware-rendered (OpenGL) via Beetle PSX HW.
    PlayStation,
    /// J2ME (Java ME): the feature-phone games. A `.jar` MIDlet suite, run by
    /// a Java VM the core starts as a child process.
    J2me,
}

/// The consoles, in the order the settings screen lists them.
pub const SYSTEMS: &[SystemId] = &[
    SystemId::Nes,
    SystemId::Gba,
    SystemId::Gb,
    SystemId::Snes,
    SystemId::Genesis,
    SystemId::MasterSystem,
    SystemId::GameGear,
    SystemId::Sg1000,
    SystemId::Arcade,
    SystemId::N64,
    SystemId::Psp,
    SystemId::PlayStation,
    SystemId::J2me,
];

impl SystemId {
    /// Spelled out, for the settings screen where there is room.
    pub fn name(self) -> &'static str {
        match self {
            SystemId::Nes => "NES / FC",
            SystemId::Gba => "Game Boy Advance",
            SystemId::Gb => "Game Boy / Color",
            SystemId::Snes => "Super Nintendo / SFC",
            SystemId::Genesis => "Sega Mega Drive / Genesis",
            SystemId::MasterSystem => "Sega Master System",
            SystemId::GameGear => "Sega Game Gear",
            SystemId::Sg1000 => "SG-1000",
            SystemId::Arcade => "Arcade (MAME)",
            SystemId::N64 => "Nintendo 64",
            SystemId::Psp => "PlayStation Portable",
            SystemId::PlayStation => "Sony PlayStation",
            SystemId::J2me => "J2ME (Java ME)",
        }
    }

    /// Short form, for the badges on library cards where the whole word would
    /// be wider than the card.
    pub fn short(self) -> &'static str {
        match self {
            SystemId::Nes => "NES",
            SystemId::Gba => "GBA",
            SystemId::Gb => "GB",
            SystemId::Snes => "SNES",
            SystemId::Genesis => "MD",
            SystemId::MasterSystem => "SMS",
            SystemId::GameGear => "GG",
            SystemId::Sg1000 => "SG",
            SystemId::Arcade => "ARC",
            SystemId::N64 => "N64",
            SystemId::Psp => "PSP",
            SystemId::PlayStation => "PS1",
            SystemId::J2me => "J2ME",
        }
    }

    /// The file extensions (lower case, no dot) that select this console.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SystemId::Nes => &["nes"],
            SystemId::Gba => &["gba"],
            SystemId::Gb => &["gb", "gbc"],
            // Super Nintendo ROM images. `.bin` is a raw Mega Drive dump and
            // stays with Genesis; a SNES `.bin` is fixed per game from the
            // card's "选择机种…" menu. `.bs`/`.st` are left out: they need a
            // BS-X / Sufami Turbo BIOS the app does not bundle.
            SystemId::Snes => &["sfc", "smc", "fig", "swc"],
            // `.bin` is the common raw Mega Drive dump; nothing else here
            // claims it, and the scanner only admits known extensions.
            SystemId::Genesis => &["md", "gen", "smd", "bin"],
            SystemId::MasterSystem => &["sms"],
            SystemId::GameGear => &["gg"],
            SystemId::Sg1000 => &["sg"],
            SystemId::Arcade => &["zip"],
            SystemId::N64 => &["z64", "n64", "v64"],
            // PSP disc/eboot images and CHD-compressed dumps. No other console
            // here claims these yet; when one does, the scanner must disambiguate.
            SystemId::Psp => &["iso", "cso", "pbp", "chd"],
            // The CD-image extensions only PS1 uses. `.iso`, `.chd`, `.pbp`
            // are shared with PSP and stay there; a wrong guess is fixed per
            // game from the card's "选择机种…" menu.
            SystemId::PlayStation => &["cue", "ccd", "toc", "m3u", "img"],
            // `.jar` is a MIDlet suite; `.kjx` is a Keitai (i-appli) archive
            // the core also accepts. `.jad` (the descriptor) is not a game.
            SystemId::J2me => &["jar", "kjx"],
        }
    }

    /// A stable string key for persistence (SQLite rows, JSON settings).
    pub fn key(self) -> &'static str {
        match self {
            SystemId::Nes => "nes",
            SystemId::Gba => "gba",
            SystemId::Gb => "gb",
            SystemId::Snes => "snes",
            SystemId::Genesis => "genesis",
            SystemId::MasterSystem => "sms",
            SystemId::GameGear => "gg",
            SystemId::Sg1000 => "sg1000",
            SystemId::Arcade => "arcade",
            SystemId::N64 => "n64",
            SystemId::Psp => "psp",
            SystemId::PlayStation => "ps1",
            SystemId::J2me => "j2me",
        }
    }

    /// Parse a console key, or `None` when it is not one we know.
    /// Case-insensitive. The one place the key → console map lives.
    pub fn parse_key(key: &str) -> Option<SystemId> {
        match key.to_ascii_lowercase().as_str() {
            "nes" => Some(SystemId::Nes),
            "gba" => Some(SystemId::Gba),
            "gb" | "gbc" => Some(SystemId::Gb),
            "snes" | "sfc" => Some(SystemId::Snes),
            "genesis" | "md" | "megadrive" => Some(SystemId::Genesis),
            "sms" => Some(SystemId::MasterSystem),
            "gg" => Some(SystemId::GameGear),
            "sg1000" | "sg" => Some(SystemId::Sg1000),
            "arcade" => Some(SystemId::Arcade),
            "n64" => Some(SystemId::N64),
            "psp" => Some(SystemId::Psp),
            "ps1" | "psx" | "playstation" => Some(SystemId::PlayStation),
            "j2me" | "java" => Some(SystemId::J2me),
            _ => None,
        }
    }

    /// Parse a [`SystemId::key`], defaulting to NES for unknown strings.
    pub fn from_key(key: &str) -> SystemId {
        Self::parse_key(key).unwrap_or(SystemId::Nes)
    }
}

/// The extension of a path, lower case and without the dot, or `""`.
pub fn extension_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The console a path is for. Unknown extensions are NES, by design.
pub fn system_for_path(path: &str) -> SystemId {
    match extension_of(path).as_str() {
        "gba" => SystemId::Gba,
        "gb" | "gbc" => SystemId::Gb,
        "sfc" | "smc" | "fig" | "swc" => SystemId::Snes,
        "md" | "gen" | "smd" | "bin" => SystemId::Genesis,
        "sms" => SystemId::MasterSystem,
        "gg" => SystemId::GameGear,
        "sg" => SystemId::Sg1000,
        "zip" => SystemId::Arcade,
        "z64" | "n64" | "v64" => SystemId::N64,
        "iso" | "cso" | "pbp" | "chd" => SystemId::Psp,
        "cue" | "ccd" | "toc" | "m3u" | "img" => SystemId::PlayStation,
        "jar" | "kjx" => SystemId::J2me,
        _ => SystemId::Nes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_choose_the_console() {
        assert_eq!(system_for_path("mario.nes"), SystemId::Nes);
        assert_eq!(system_for_path("pokemon.GBA"), SystemId::Gba);
        assert_eq!(system_for_path("tetris.gb"), SystemId::Gb);
        assert_eq!(system_for_path("tetris.gbc"), SystemId::Gb);
        assert_eq!(system_for_path("mario.sfc"), SystemId::Snes);
        assert_eq!(system_for_path("MARIO.SMC"), SystemId::Snes);
        assert_eq!(system_for_path("mario.fig"), SystemId::Snes);
        assert_eq!(system_for_path("mario.swc"), SystemId::Snes);
        assert_eq!(system_for_path("sonic.md"), SystemId::Genesis);
        assert_eq!(system_for_path("sonic.gen"), SystemId::Genesis);
        assert_eq!(system_for_path("sonic.smd"), SystemId::Genesis);
        assert_eq!(system_for_path("sonic.bin"), SystemId::Genesis);
        assert_eq!(system_for_path("alex.sms"), SystemId::MasterSystem);
        assert_eq!(system_for_path("columns.gg"), SystemId::GameGear);
        assert_eq!(system_for_path("girls.sg"), SystemId::Sg1000);
        assert_eq!(system_for_path("puckman.zip"), SystemId::Arcade);
        assert_eq!(system_for_path("mario.z64"), SystemId::N64);
        assert_eq!(system_for_path("MARIO.N64"), SystemId::N64);
        assert_eq!(system_for_path("mario.v64"), SystemId::N64);
        assert_eq!(system_for_path("crisis.iso"), SystemId::Psp);
        assert_eq!(system_for_path("CRISIS.CSO"), SystemId::Psp);
        assert_eq!(system_for_path("homebrew.pbp"), SystemId::Psp);
        assert_eq!(system_for_path("dumped.chd"), SystemId::Psp);
        assert_eq!(system_for_path("ff7.cue"), SystemId::PlayStation);
        assert_eq!(system_for_path("FF7.CCD"), SystemId::PlayStation);
        assert_eq!(system_for_path("disc.toc"), SystemId::PlayStation);
        assert_eq!(system_for_path("set.m3u"), SystemId::PlayStation);
        assert_eq!(system_for_path("track.img"), SystemId::PlayStation);
        assert_eq!(system_for_path("pileup.jar"), SystemId::J2me);
        assert_eq!(system_for_path("PILEUP.JAR"), SystemId::J2me);
        assert_eq!(system_for_path("keitai.kjx"), SystemId::J2me);
    }

    #[test]
    fn parse_key_round_trips_every_console() {
        for system in SYSTEMS {
            assert_eq!(SystemId::parse_key(system.key()), Some(*system));
        }
        assert_eq!(SystemId::parse_key("GBC"), Some(SystemId::Gb));
        assert_eq!(SystemId::parse_key("MEGADRIVE"), Some(SystemId::Genesis));
        assert_eq!(SystemId::parse_key("md"), Some(SystemId::Genesis));
        assert_eq!(SystemId::parse_key("wonderswan"), None);
    }

    #[test]
    fn an_unknown_file_is_a_nes_game() {
        // The old behaviour, kept on purpose: a file with no extension gets a
        // clear failure from a NES core rather than a "don't know what to do".
        assert_eq!(system_for_path("mystery"), SystemId::Nes);
        assert_eq!(system_for_path("readme.txt"), SystemId::Nes);
    }

    #[test]
    fn a_dot_in_a_folder_does_not_look_like_an_extension() {
        assert_eq!(extension_of("/roms/nes.v2/mario"), "");
        assert_eq!(system_for_path("/roms/nes.v2/mario"), SystemId::Nes);
    }
}
