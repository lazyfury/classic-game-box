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
}

/// The consoles, in the order the settings screen lists them.
pub const SYSTEMS: &[SystemId] = &[SystemId::Nes, SystemId::Gba, SystemId::Gb];

impl SystemId {
    /// Spelled out, for the settings screen where there is room.
    pub fn name(self) -> &'static str {
        match self {
            SystemId::Nes => "NES / FC",
            SystemId::Gba => "Game Boy Advance",
            SystemId::Gb => "Game Boy / Color",
        }
    }

    /// Short form, for the badges on library cards where the whole word would
    /// be wider than the card.
    pub fn short(self) -> &'static str {
        match self {
            SystemId::Nes => "NES",
            SystemId::Gba => "GBA",
            SystemId::Gb => "GB",
        }
    }

    /// The file extensions (lower case, no dot) that select this console.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SystemId::Nes => &["nes"],
            SystemId::Gba => &["gba"],
            SystemId::Gb => &["gb", "gbc"],
        }
    }

    /// A stable string key for persistence (SQLite rows, JSON settings).
    pub fn key(self) -> &'static str {
        match self {
            SystemId::Nes => "nes",
            SystemId::Gba => "gba",
            SystemId::Gb => "gb",
        }
    }

    /// Parse a [`SystemId::key`], defaulting to NES for unknown strings.
    pub fn from_key(key: &str) -> SystemId {
        match key {
            "gba" => SystemId::Gba,
            "gb" | "gbc" => SystemId::Gb,
            _ => SystemId::Nes,
        }
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
