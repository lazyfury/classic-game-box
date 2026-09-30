//! Pure functions projecting library/core state into `ViewModel` rows.

use super::*;

/// Order games the way the library shows them: pinned first, then the sort
/// key within each group. Pure, so the ordering can be tested without a window.
pub(crate) fn order_games(games: &mut [Game], key: SortKey, desc: bool) {
    games.sort_by(|a, b| {
        let order = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::LastPlayed => a.last_played_at.cmp(&b.last_played_at),
            SortKey::Playtime => a.play_seconds.cmp(&b.play_seconds),
            SortKey::Added => a.added_at.cmp(&b.added_at),
        };
        let order = if desc { order.reverse() } else { order };
        b.pinned.cmp(&a.pinned).then(order)
    });
}

/// How many games the library holds per console, in `SYSTEMS` order, keeping
/// only the consoles that have games. Pure, so the tally can be tested without
/// a window.
pub(crate) fn system_counts(games: &[Game]) -> Vec<SystemCount> {
    cgb_systems::SYSTEMS
        .iter()
        .filter_map(|&system| {
            let count = games.iter().filter(|game| game.system == system).count();
            (count > 0).then_some(SystemCount { system, count })
        })
        .collect()
}

/// Whether `game` matches a library search query.
///
/// Whitespace-separated tokens are AND-ed. A token starting with `#` matches a
/// tag (substring, case-insensitive); any other token matches the display name
/// (substring, case-insensitive). So `mario #rpg` finds names containing
/// "mario" that also carry an "rpg" tag.
pub(crate) fn game_matches_search(game: &Game, query: &str) -> bool {
    query.split_whitespace().all(|token| {
        if let Some(tag) = token.strip_prefix('#') {
            let tag = tag.to_lowercase();
            tag.is_empty()
                || game
                    .tags
                    .iter()
                    .any(|candidate| candidate.to_lowercase().contains(&tag))
        } else {
            game.name.to_lowercase().contains(&token.to_lowercase())
        }
    })
}

/// Project a library row into the view model's row.
pub(crate) fn game_row(game: Game, cover: Option<FrameHandle>) -> GameRow {
    GameRow {
        id: game.id,
        name: game.name,
        file_name: game.file_name,
        system: game.system,
        path: game.path,
        size: game.size,
        pinned: game.pinned,
        play_count: game.play_count,
        play_seconds: game.play_seconds,
        last_played_at: game.last_played_at,
        tags: game.tags,
        screenshots: game.screenshots,
        cover,
    }
}

/// One row per joypad button that has keys bound, for the settings page.
pub(crate) fn binding_rows(bindings: &KeyboardBindings) -> Vec<BindingRow> {
    JoypadButton::ALL
        .iter()
        .filter_map(|button| {
            let keys: Vec<String> = bindings
                .entries()
                .iter()
                .filter(|(_, bound)| bound == button)
                .map(|(key, _)| key_label(*key))
                .collect();
            (!keys.is_empty()).then(|| BindingRow {
                button: button.label().to_string(),
                keys: keys.join(" / "),
            })
        })
        .collect()
}

/// A bound key's printable name for the bindings list.
pub(crate) fn key_label(key: cgb_input::Key) -> String {
    use cgb_input::Key;
    match key {
        Key::Character(c) => c.to_ascii_uppercase().to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Space => "Space".to_string(),
        Key::ArrowUp => "↑".to_string(),
        Key::ArrowDown => "↓".to_string(),
        Key::ArrowLeft => "←".to_string(),
        Key::ArrowRight => "→".to_string(),
    }
}

/// The status line after adding games: how many were copied and the first
/// reason any were skipped.
pub(crate) fn import_status(report: &ImportReport) -> String {
    let copied = report.copied_count();
    let skipped = report.skipped_count();
    let mut parts = Vec::new();
    if copied > 0 {
        parts.push(format!("已添加 {copied} 个游戏到游戏库"));
    }
    if let Some((path, error)) = report.failed.first() {
        parts.push(format!("拷贝失败 {}（{error}）", path.display()));
    }
    if parts.is_empty() {
        return match (report.unknown.first(), report.already_inside.is_empty()) {
            (Some(path), _) => format!("跳过不认识的 ROM：{}", path.display()),
            (None, false) => "这些游戏已经在游戏库里了".to_string(),
            _ => "没有新增游戏".to_string(),
        };
    }
    let mut message = parts.join("，");
    if skipped > 0 {
        message.push_str(&format!("（跳过 {skipped} 个）"));
    }
    message
}

/// The save-state hotkey for a key, matching the old front end's layout: `F5`
/// quick-saves, `F6` quick-loads, `F1`–`F3` save slots 1–3, and
/// `Shift`+`F1`–`F3` loads them. `F11` toggles fullscreen.
pub(crate) fn state_shortcut(key: Key, shift: bool) -> Option<Action> {
    match key {
        Key::F11 => Some(Action::ToggleFullscreen),
        Key::F5 => Some(Action::SaveState(0)),
        Key::F6 => Some(Action::LoadState(0)),
        Key::F1 if shift => Some(Action::LoadState(1)),
        Key::F2 if shift => Some(Action::LoadState(2)),
        Key::F3 if shift => Some(Action::LoadState(3)),
        Key::F1 => Some(Action::SaveState(1)),
        Key::F2 => Some(Action::SaveState(2)),
        Key::F3 => Some(Action::SaveState(3)),
        _ => None,
    }
}
