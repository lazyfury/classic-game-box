//! The diesel schema for the game library.
//!
//! The library is one self-contained SQLite database, and this is the whole
//! model: ROMs, screenshots, save states, cheats and tags. Every child row
//! points at `games(id)` with `ON DELETE CASCADE`, so forgetting a game forgets
//! everything the app knows about it in one statement.
//!
//! `save_states` / `cheats` are part of the schema from the start even though
//! the app still writes their bytes through the file-system helpers — the
//! tables are reconciled from disk so the model stays complete.

diesel::table! {
    games (id) {
        id -> BigInt,
        /// ROM path relative to the library root (portable across machines).
        key -> Text,
        file_name -> Text,
        name -> Text,
        system -> Text,
        /// Per-game core override; `None` follows the console's pick.
        core_key -> Nullable<Text>,
        size -> BigInt,
        mtime_ms -> BigInt,
        added_at -> BigInt,
        last_played_at -> BigInt,
        play_count -> BigInt,
        play_seconds -> BigInt,
        pinned -> Bool,
        /// The screenshot used as the cover, if any.
        cover_id -> Nullable<BigInt>,
    }
}

diesel::table! {
    screenshots (id) {
        id -> BigInt,
        game_id -> BigInt,
        /// File name inside the library's `screenshots/` directory.
        file -> Text,
        created_at -> BigInt,
        width -> BigInt,
        height -> BigInt,
    }
}

diesel::table! {
    save_states (id) {
        id -> BigInt,
        game_id -> BigInt,
        /// A state is not portable between cores, so the core is part of it.
        core_key -> Text,
        /// `manual` (fixed slots) or `quick` (the rolling stack).
        kind -> Text,
        /// `1..=9` for manual, `0..=2` for quick (newest is `0`).
        slot -> BigInt,
        /// File name inside the library's `saves/` directory.
        state_file -> Text,
        /// The state's thumbnail, if one was written.
        thumb_file -> Nullable<Text>,
        modified_ms -> BigInt,
    }
}

diesel::table! {
    cheats (id) {
        id -> BigInt,
        game_id -> BigInt,
        /// File name inside the library's `cheats/` directory.
        file -> Text,
    }
}

diesel::table! {
    tags (id) {
        id -> BigInt,
        name -> Text,
    }
}

diesel::table! {
    game_tags (game_id, tag_id) {
        game_id -> BigInt,
        tag_id -> BigInt,
    }
}

diesel::joinable!(screenshots -> games (game_id));
diesel::joinable!(save_states -> games (game_id));
diesel::joinable!(cheats -> games (game_id));
diesel::joinable!(game_tags -> games (game_id));
diesel::joinable!(game_tags -> tags (tag_id));

diesel::allow_tables_to_appear_in_same_query!(
    games,
    screenshots,
    save_states,
    cheats,
    tags,
    game_tags,
);
