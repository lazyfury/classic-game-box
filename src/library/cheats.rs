//! Cheats: a per-game list of codes, in RetroArch's `.cht` text format.
//!
//! A cheat is a `(description, code, enabled)` triple; the core interprets the
//! code string (GameShark, GameGenie, RAW, …) when the frontend hands it to
//! `retro_cheat_set`. The file is RetroArch's, so a list copied from anywhere
//! that writes `.cht` loads as-is.

use std::path::Path;

use super::error::LibraryError;

/// One cheat in a game's list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cheat {
    pub desc: String,
    pub code: String,
    pub enabled: bool,
}

/// Read a `.cht` file, or an empty list when it is missing or unreadable.
pub fn load_cheats(path: &Path) -> Vec<Cheat> {
    std::fs::read_to_string(path)
        .map(|text| parse_cht(&text))
        .unwrap_or_default()
}

/// Write a `.cht` file, creating the parent directory if needed.
pub fn save_cheats(path: &Path, cheats: &[Cheat]) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, write_cht(cheats))?;
    Ok(())
}

/// Parse RetroArch's `.cht` text into a list.
pub fn parse_cht(text: &str) -> Vec<Cheat> {
    let mut cheats: Vec<Cheat> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        let Some(rest) = key.strip_prefix("cheat") else {
            continue;
        };
        // `rest` is `<index>_<field>`, e.g. `0_code`.
        let Some((index, field)) = rest.split_once('_') else {
            continue;
        };
        let Ok(index) = index.parse::<usize>() else {
            continue;
        };
        while cheats.len() <= index {
            cheats.push(Cheat::default());
        }
        match field {
            "desc" => cheats[index].desc = value,
            "code" => cheats[index].code = value,
            "enable" => cheats[index].enabled = matches!(value.as_str(), "true" | "1" | "yes"),
            _ => {}
        }
    }
    cheats
        .into_iter()
        .filter(|cheat| !cheat.code.is_empty())
        .collect()
}

/// Render a list as RetroArch's `.cht` text.
pub fn write_cht(cheats: &[Cheat]) -> String {
    let mut out = format!("cheats = {}\n\n", cheats.len());
    for (index, cheat) in cheats.iter().enumerate() {
        out.push_str(&format!("cheat{index}_desc = \"{}\"\n", cheat.desc));
        out.push_str(&format!("cheat{index}_code = \"{}\"\n", cheat.code));
        out.push_str(&format!(
            "cheat{index}_enable = {}\n\n",
            if cheat.enabled { "true" } else { "false" }
        ));
    }
    out
}

/// Trim surrounding double quotes, if any.
fn unquote(value: &str) -> String {
    value.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cht_round_trips() {
        let cheats = vec![
            Cheat {
                desc: "Infinite Lives".to_string(),
                code: "AAAA-AAAA".to_string(),
                enabled: true,
            },
            Cheat {
                desc: "Max Coins".to_string(),
                code: "BBBB-BBBB".to_string(),
                enabled: false,
            },
        ];
        let text = write_cht(&cheats);
        assert_eq!(parse_cht(&text), cheats);
    }

    #[test]
    fn parsing_tolerates_spacing_quotes_and_missing_entries() {
        let text = "cheats = 2\ncheat0_desc = No Quotes\ncheat0_code = \"X-Y\"\n\
                    cheat1_code = Z-W\ncheat1_enable = 1\n";
        let cheats = parse_cht(text);
        assert_eq!(cheats.len(), 2);
        assert_eq!(cheats[0].desc, "No Quotes");
        assert_eq!(cheats[0].code, "X-Y");
        assert!(!cheats[0].enabled);
        assert_eq!(cheats[1].code, "Z-W");
        assert!(cheats[1].enabled);
    }

    #[test]
    fn codes_without_a_code_are_dropped() {
        // A `desc` with no `code` is not a cheat.
        let text = "cheat0_desc = \"orphan\"\ncheat1_code = \"KEEP\"\n";
        let cheats = parse_cht(text);
        assert_eq!(cheats.len(), 1);
        assert_eq!(cheats[0].code, "KEEP");
    }
}
