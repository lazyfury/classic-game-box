//! The game library: a SQLite (diesel) model of the ROM folders, plus a folder
//! scanner.
//!
//! The folder is the truth about what exists, but the database is the model:
//! the display name, the pin, the play count and time, the screenshots and the
//! cover — none of which a directory listing can express. A **manual** rescan
//! (`Library::sync`, driven from the UI) reconciles the two; nothing rescans on
//! its own.
//!
//! ROM paths are stored **relative to the library root**, so the folder stays
//! portable between machines and operating systems; [`Library::games`] hands
//! them back as absolute paths. A rescan never destroys a row unless the file is
//! provably gone (its directory exists but the file does not), so a library that
//! simply is not mounted here survives a scan intact.
//!
//! The schema is the whole model — see [`super::schema`]. There is deliberately
//! no migration chain: a database whose `user_version` is not this build's is
//! wiped and recreated.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use cgb_libretro::{extension_of, system_for_path, SystemId};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

use super::error::LibraryError;
use super::schema::{cheats, game_tags, games, save_states, screenshots, tags};

/// The schema this build writes. A database with any other version is wiped.
const SCHEMA_VERSION: i64 = 1;

/// The whole model, as a fresh database creates it (see [`super::schema`]).
const SCHEMA_SQL: &str = "
CREATE TABLE games (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    key            TEXT    NOT NULL UNIQUE,
    file_name      TEXT    NOT NULL,
    name           TEXT    NOT NULL,
    system         TEXT    NOT NULL,
    core_key       TEXT,
    size           INTEGER NOT NULL,
    mtime_ms       INTEGER NOT NULL DEFAULT 0,
    added_at       INTEGER NOT NULL,
    last_played_at INTEGER NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    play_seconds   INTEGER NOT NULL DEFAULT 0,
    pinned         INTEGER NOT NULL DEFAULT 0,
    cover_id       INTEGER
);
CREATE TABLE screenshots (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    file       TEXT    NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    width      INTEGER NOT NULL DEFAULT 0,
    height     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE save_states (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    core_key    TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    slot        INTEGER NOT NULL,
    state_file  TEXT    NOT NULL,
    thumb_file  TEXT,
    modified_ms INTEGER NOT NULL DEFAULT 0,
    UNIQUE (game_id, core_key, kind, slot)
);
CREATE TABLE cheats (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    file    TEXT    NOT NULL UNIQUE
);
CREATE TABLE tags (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT    NOT NULL COLLATE NOCASE UNIQUE
);
CREATE TABLE game_tags (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (game_id, tag_id)
);
";

/// One row of `PRAGMA user_version`.
#[derive(QueryableByName)]
struct UserVersion {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    user_version: i64,
}

/// One ROM found on disk. What a scan produces; the database reconciles it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskGame {
    /// Absolute path from the scan; [`Library::sync`] stores it relative to the
    /// library root.
    pub path: String,
    /// The file name with its extension (`mario.nes`).
    pub file_name: String,
    /// Which console the extension says it is for.
    pub system: SystemId,
    /// File size in bytes.
    pub size: u64,
    /// File modification time, milliseconds since the Unix epoch (0 if unknown).
    pub mtime_ms: i64,
}

impl DiskGame {
    /// A scan entry for a ROM path, or `None` if its extension is not a
    /// console this app knows.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().into_owned();
        let extension = extension_of(&path_str);
        let known = cgb_libretro::SYSTEMS
            .iter()
            .any(|system| system.extensions().contains(&extension.as_str()));
        if !known {
            return None;
        }
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_str.clone());
        let (size, mtime_ms) = std::fs::metadata(path)
            .map(|meta| (meta.len(), mtime_millis(&meta)))
            .unwrap_or((0, 0));
        Some(Self {
            path: path_str.clone(),
            file_name,
            system: system_for_path(&path_str),
            size,
            mtime_ms,
        })
    }
}

/// One game in the library: a database row, with everything the app knows
/// about it. Built by [`Library::games`]; never constructed by hand in the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    /// Row id; screenshots reference it.
    pub id: i64,
    /// Absolute ROM path. The database stores it relative to the library root;
    /// [`Library::games`] resolves it back against the current root.
    pub path: String,
    /// The actual file name (`mario.nes`), never edited by a rename.
    pub file_name: String,
    /// The display name, initialised from the file stem and editable.
    pub name: String,
    pub system: SystemId,
    /// The core this one game runs, by manifest key, or `None` to follow the
    /// console's pick. Set from a card's "选择核心…" menu.
    pub core: Option<String>,
    pub size: u64,
    pub mtime_ms: i64,
    pub added_at: i64,
    pub last_played_at: i64,
    pub play_count: i64,
    pub play_seconds: i64,
    pub pinned: bool,
    /// The screenshot used as the cover, if any.
    pub cover: Option<i64>,
    /// How many screenshots have been taken of it.
    pub screenshots: i64,
    /// The player's labels, alphabetically and without duplicates.
    pub tags: Vec<String>,
}

impl Game {
    /// A metadata-less view of a disk game, for when the database could not be
    /// opened. Everything the model would fill in is empty.
    pub fn from_disk(disk: &DiskGame) -> Self {
        Self {
            id: -1,
            path: disk.path.clone(),
            file_name: disk.file_name.clone(),
            name: stem_of(&disk.file_name),
            system: disk.system,
            core: None,
            size: disk.size,
            mtime_ms: disk.mtime_ms,
            added_at: 0,
            last_played_at: 0,
            play_count: 0,
            play_seconds: 0,
            pinned: false,
            cover: None,
            screenshots: 0,
            tags: Vec::new(),
        }
    }
}

/// One screenshot row, relative to the library's screenshot directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub id: i64,
    pub game_id: i64,
    /// The file name inside the screenshots directory.
    pub file: String,
    pub created_at: i64,
    pub width: i64,
    pub height: i64,
}

/// Which save-state stack a slot belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveKind {
    /// A fixed slot `1..=9`.
    Manual,
    /// The rolling quick stack, `0` (newest) `..=2`.
    Quick,
}

impl SaveKind {
    /// The stored key (and the file-name infix).
    pub fn key(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Quick => "quick",
        }
    }
}

/// One save-state file found on disk, before it is linked to a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskSave {
    /// The ROM file name the state's name is built from (`mario.nes`).
    pub rom_file: String,
    pub core_key: String,
    pub kind: SaveKind,
    pub slot: u8,
    /// File name inside the `saves/` directory.
    pub state_file: String,
    /// Its thumbnail, if one was written.
    pub thumb_file: Option<String>,
    pub modified_ms: i64,
}

/// One cheat file found on disk (named after the ROM's file stem).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskCheat {
    /// The game file stem (`mario`), which is how the file names its game.
    pub stem: String,
    /// File name inside the `cheats/` directory.
    pub file: String,
}

/// A `games` row as diesel loads it.
#[derive(Queryable, Selectable)]
#[diesel(table_name = games)]
struct GameRow {
    id: i64,
    key: String,
    file_name: String,
    name: String,
    system: String,
    core_key: Option<String>,
    size: i64,
    mtime_ms: i64,
    added_at: i64,
    last_played_at: i64,
    play_count: i64,
    play_seconds: i64,
    pinned: bool,
    cover_id: Option<i64>,
}

/// A `screenshots` row as diesel loads it.
#[derive(Queryable, Selectable)]
#[diesel(table_name = screenshots)]
struct ScreenshotRow {
    id: i64,
    game_id: i64,
    file: String,
    created_at: i64,
    width: i64,
    height: i64,
}

impl ScreenshotRow {
    fn into_domain(self) -> Screenshot {
        Screenshot {
            id: self.id,
            game_id: self.game_id,
            file: self.file,
            created_at: self.created_at,
            width: self.width,
            height: self.height,
        }
    }
}

/// The SQLite-backed library.
pub struct Library {
    conn: RefCell<SqliteConnection>,
    /// The library folder the database lives in (the database's parent). ROM
    /// paths are stored relative to it.
    root: PathBuf,
    /// Where screenshot files live; a sibling of the database.
    screenshots_dir: PathBuf,
    /// Where save-state files live.
    saves_dir: PathBuf,
    /// Where cheat files live.
    cheats_dir: PathBuf,
    /// Whether opening discarded a database written by another version.
    reset: bool,
}

impl Library {
    /// Open the database at `path`, creating the schema if it is missing or was
    /// written by a different version (the old contents are discarded — there is
    /// no migration).
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = SqliteConnection::establish(&path.to_string_lossy())?;
        conn.batch_execute("PRAGMA foreign_keys = ON;")?;
        let reset = read_version(&mut conn)? != SCHEMA_VERSION;
        if reset {
            reset_schema(&mut conn)?;
        }
        let root = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let screenshots_dir = root.join("screenshots");
        let saves_dir = root.join("saves");
        let cheats_dir = root.join("cheats");
        Ok(Self {
            conn: RefCell::new(conn),
            root,
            screenshots_dir,
            saves_dir,
            cheats_dir,
            reset,
        })
    }

    /// Whether opening this library discarded an outdated database (so the app
    /// should rescan the folder once to repopulate it).
    pub fn was_reset(&self) -> bool {
        self.reset
    }

    /// The stored form of a ROM path: relative to the library root when the
    /// file is inside it, otherwise the absolute path unchanged (an added ROM
    /// may live outside the library). Storing the relative form is what makes
    /// the library folder portable.
    fn key(&self, path: &str) -> String {
        key_for(&self.root, path)
    }

    // -- reconciliation -----------------------------------------------------

    /// Reconcile the database with a fresh scan.
    ///
    /// A key already known keeps its name, pin, play statistics, screenshots,
    /// saves and cover; only the file name, size and modification time are
    /// refreshed. A key that appeared is inserted with its display name taken
    /// from the file stem.
    ///
    /// Deletion is **non-destructive**: a row is forgotten only when its file is
    /// provably gone (its directory exists but the file does not). A key whose
    /// directory is missing is kept — the library may simply not be mounted
    /// here, or the row may have been written by another OS.
    ///
    /// Returns how many rows were removed.
    pub fn sync(&self, found: &[DiskGame]) -> Result<usize, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let known: HashSet<String> = games::table
            .select(games::key)
            .load::<String>(&mut *conn)?
            .into_iter()
            .collect();
        let now = now_millis();
        for game in found {
            let key = self.key(&game.path);
            if known.contains(&key) {
                diesel::update(games::table.filter(games::key.eq(&key)))
                    .set((
                        games::file_name.eq(&game.file_name),
                        games::size.eq(game.size as i64),
                        games::mtime_ms.eq(game.mtime_ms),
                    ))
                    .execute(&mut *conn)?;
            } else {
                diesel::insert_into(games::table)
                    .values((
                        games::key.eq(&key),
                        games::file_name.eq(&game.file_name),
                        games::name.eq(stem_of(&game.file_name)),
                        games::system.eq(game.system.key()),
                        games::size.eq(game.size as i64),
                        games::mtime_ms.eq(game.mtime_ms),
                        games::added_at.eq(now),
                    ))
                    .execute(&mut *conn)?;
            }
        }

        let keep: HashSet<String> = found.iter().map(|game| self.key(&game.path)).collect();
        let stored: Vec<String> = games::table.select(games::key).load(&mut *conn)?;
        let mut removed = 0;
        for key in stored {
            if keep.contains(&key) {
                continue;
            }
            let file = PathBuf::from(resolve_against(&self.root, &key));
            let gone = file.parent().is_some_and(|dir| dir.exists()) && !file.exists();
            if gone {
                self.remove_key(&mut conn, &key)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Reconcile the `save_states` table with the files on disk.
    ///
    /// Saves have no metadata beyond the files themselves, so the scan is the
    /// whole truth: the table is replaced. A state whose ROM is not in the
    /// library stays on disk but is not linked.
    pub fn sync_saves(&self, saves: &[DiskSave]) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let by_name: HashMap<String, i64> = games::table
            .select((games::file_name, games::id))
            .load::<(String, i64)>(&mut *conn)?
            .into_iter()
            .collect();
        conn.transaction::<_, LibraryError, _>(|conn| {
            diesel::delete(save_states::table).execute(conn)?;
            for save in saves {
                let Some(&game_id) = by_name.get(&save.rom_file) else {
                    continue;
                };
                diesel::insert_into(save_states::table)
                    .values((
                        save_states::game_id.eq(game_id),
                        save_states::core_key.eq(&save.core_key),
                        save_states::kind.eq(save.kind.key()),
                        save_states::slot.eq(save.slot as i64),
                        save_states::state_file.eq(&save.state_file),
                        save_states::thumb_file.eq(save.thumb_file.clone()),
                        save_states::modified_ms.eq(save.modified_ms),
                    ))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Reconcile the `cheats` table with the files on disk. Like saves, cheats
    /// are named after their game, so the table is replaced from the scan.
    pub fn sync_cheats(&self, found: &[DiskCheat]) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let by_stem: HashMap<String, i64> = games::table
            .select((games::file_name, games::id))
            .load::<(String, i64)>(&mut *conn)?
            .into_iter()
            .map(|(name, id)| (stem_of(&name), id))
            .collect();
        conn.transaction::<_, LibraryError, _>(|conn| {
            diesel::delete(cheats::table).execute(conn)?;
            for cheat in found {
                let Some(&game_id) = by_stem.get(&cheat.stem) else {
                    continue;
                };
                diesel::insert_into(cheats::table)
                    .values((cheats::game_id.eq(game_id), cheats::file.eq(&cheat.file)))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Every known game, ordered the way the library shows them: pinned first,
    /// then most recently played, then by name.
    pub fn games(&self) -> Result<Vec<Game>, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let rows: Vec<GameRow> = games::table
            .select(GameRow::as_select())
            .order((
                games::pinned.desc(),
                games::last_played_at.desc(),
                games::name.asc(),
            ))
            .load(&mut *conn)?;
        let counts: HashMap<i64, i64> = screenshots::table
            .group_by(screenshots::game_id)
            .select((screenshots::game_id, diesel::dsl::count_star()))
            .load::<(i64, i64)>(&mut *conn)?
            .into_iter()
            .collect();
        let mut tags_by_game = tags_by_game(&mut conn)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(Game {
                id: row.id,
                path: resolve_against(&self.root, &row.key),
                file_name: row.file_name,
                name: row.name,
                system: SystemId::from_key(&row.system),
                core: row.core_key,
                size: row.size.max(0) as u64,
                mtime_ms: row.mtime_ms,
                added_at: row.added_at,
                last_played_at: row.last_played_at,
                play_count: row.play_count,
                play_seconds: row.play_seconds,
                pinned: row.pinned,
                cover: row.cover_id,
                screenshots: counts.get(&row.id).copied().unwrap_or(0),
                tags: tags_by_game.remove(&row.id).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Every known game's stored key (the reconciliation set).
    #[cfg(test)]
    fn paths(&self) -> Result<HashSet<String>, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        Ok(games::table
            .select(games::key)
            .load::<String>(&mut *conn)?
            .into_iter()
            .collect())
    }

    // -- per-game metadata --------------------------------------------------

    /// Pin or unpin a game.
    pub fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set(games::pinned.eq(pinned))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Remember the console this game runs as, overriding the one its file
    /// extension suggests (a `.chd` can be a PlayStation or a PSP disc). A
    /// rescan never overwrites it.
    pub fn set_system(&self, path: &str, system: SystemId) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set(games::system.eq(system.key()))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Remember the core this one game runs, by manifest key, overriding the
    /// console's pick. `None` clears the override. A rescan never overwrites it.
    pub fn set_core(&self, path: &str, core: Option<&str>) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set(games::core_key.eq(core))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Record that a game was run: bump its count and stamp the time. Does
    /// nothing for a path that is not in the library.
    pub fn note_played(&self, path: &str, at: i64) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set((
                games::last_played_at.eq(at),
                games::play_count.eq(games::play_count + 1),
            ))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Add whole seconds to a game's total play time. A non-positive or absent
    /// game is ignored.
    pub fn note_playtime(&self, path: &str, seconds: i64) -> Result<(), LibraryError> {
        if seconds <= 0 {
            return Ok(());
        }
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set(games::play_seconds.eq(games::play_seconds + seconds))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Rename a game's display name. The file on disk is untouched.
    pub fn rename(&self, path: &str, name: &str) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        diesel::update(games::table.filter(games::key.eq(self.key(path))))
            .set(games::name.eq(name))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Replace a game's labels. A word is shared if it already exists
    /// (case-insensitively) and created otherwise; words nothing points at are
    /// pruned, so the table cannot grow rows the UI can never show.
    pub fn set_tags(&self, path: &str, tags: &[String]) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let Some(id) = game_id_by_key(&mut conn, &self.key(path))? else {
            return Ok(());
        };
        let wanted = normalise_tags(tags);
        conn.transaction::<_, LibraryError, _>(|conn| {
            diesel::delete(game_tags::table.filter(game_tags::game_id.eq(id))).execute(conn)?;
            for name in &wanted {
                diesel::insert_into(tags::table)
                    .values(tags::name.eq(name))
                    .on_conflict(tags::name)
                    .do_nothing()
                    .execute(conn)?;
                let tag_id: i64 = tags::table
                    .filter(tags::name.eq(name))
                    .select(tags::id)
                    .first(conn)?;
                diesel::insert_into(game_tags::table)
                    .values((game_tags::game_id.eq(id), game_tags::tag_id.eq(tag_id)))
                    .on_conflict_do_nothing()
                    .execute(conn)?;
            }
            conn.batch_execute("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM game_tags)")?;
            Ok(())
        })?;
        Ok(())
    }

    /// Forget a ROM: its screenshots (rows and files), saves and cheats go with
    /// it.
    pub fn remove(&self, path: &str) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        self.remove_key(&mut conn, &self.key(path))
    }

    /// Forget the game stored under `key`, deleting its screenshot files.
    fn remove_key(&self, conn: &mut SqliteConnection, key: &str) -> Result<(), LibraryError> {
        if let Some(id) = game_id_by_key(conn, key)? {
            self.delete_screenshot_files(conn, id)?;
            self.delete_save_files(conn, id)?;
            self.delete_cheat_files(conn, id)?;
        }
        diesel::delete(games::table.filter(games::key.eq(key))).execute(conn)?;
        Ok(())
    }

    // -- screenshots --------------------------------------------------------

    /// The directory screenshot files live in.
    pub fn screenshots_dir(&self) -> &Path {
        &self.screenshots_dir
    }

    /// Every screenshot, newest first.
    pub fn screenshots(&self) -> Result<Vec<Screenshot>, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let rows: Vec<ScreenshotRow> = screenshots::table
            .select(ScreenshotRow::as_select())
            .order((screenshots::created_at.desc(), screenshots::id.desc()))
            .load(&mut *conn)?;
        Ok(rows.into_iter().map(ScreenshotRow::into_domain).collect())
    }

    /// The file a screenshot lives in, or `None` for an unknown id.
    pub fn screenshot_path(&self, id: i64) -> Result<Option<PathBuf>, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let file: Option<String> = screenshots::table
            .filter(screenshots::id.eq(id))
            .select(screenshots::file)
            .first(&mut *conn)
            .optional()?;
        Ok(file.map(|file| self.screenshots_dir.join(file)))
    }

    /// Write a PNG and file it under a game. `as_cover` forces the new picture
    /// to be the cover; otherwise the game's first screenshot becomes it.
    /// Returns `None` when the game is not in the library.
    pub fn save_screenshot(
        &self,
        game_path: &str,
        png: &[u8],
        width: i64,
        height: i64,
        as_cover: bool,
    ) -> Result<Option<Screenshot>, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let Some(game_id) = game_id_by_key(&mut conn, &self.key(game_path))? else {
            return Ok(None);
        };
        std::fs::create_dir_all(&self.screenshots_dir)?;
        let file = format!("{}-{:08x}.png", now_millis(), unique_suffix());
        std::fs::write(self.screenshots_dir.join(&file), png)?;

        let count: i64 = screenshots::table
            .filter(screenshots::game_id.eq(game_id))
            .count()
            .get_result(&mut *conn)?;
        let cover = as_cover || count == 0;
        let created = now_millis();
        diesel::insert_into(screenshots::table)
            .values((
                screenshots::game_id.eq(game_id),
                screenshots::file.eq(&file),
                screenshots::created_at.eq(created),
                screenshots::width.eq(width),
                screenshots::height.eq(height),
            ))
            .execute(&mut *conn)?;
        let id: i64 = screenshots::table
            .filter(screenshots::file.eq(&file))
            .select(screenshots::id)
            .first(&mut *conn)?;
        if cover {
            diesel::update(games::table.filter(games::id.eq(game_id)))
                .set(games::cover_id.eq(Some(id)))
                .execute(&mut *conn)?;
        }
        Ok(Some(Screenshot {
            id,
            game_id,
            file,
            created_at: created,
            width,
            height,
        }))
    }

    /// Make a screenshot its game's cover.
    pub fn set_cover(&self, id: i64) -> Result<(), LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let game_id: i64 = screenshots::table
            .filter(screenshots::id.eq(id))
            .select(screenshots::game_id)
            .first(&mut *conn)?;
        diesel::update(games::table.filter(games::id.eq(game_id)))
            .set(games::cover_id.eq(Some(id)))
            .execute(&mut *conn)?;
        Ok(())
    }

    /// Delete a screenshot (row and file). Returns false for an unknown id.
    pub fn remove_screenshot(&self, id: i64) -> Result<bool, LibraryError> {
        let mut conn = self.conn.borrow_mut();
        let file: Option<String> = screenshots::table
            .filter(screenshots::id.eq(id))
            .select(screenshots::file)
            .first(&mut *conn)
            .optional()?;
        let Some(file) = file else {
            return Ok(false);
        };
        let _ = std::fs::remove_file(self.screenshots_dir.join(&file));
        diesel::update(games::table.filter(games::cover_id.eq(id)))
            .set(games::cover_id.eq(None::<i64>))
            .execute(&mut *conn)?;
        diesel::delete(screenshots::table.filter(screenshots::id.eq(id))).execute(&mut *conn)?;
        Ok(true)
    }

    /// Delete every screenshot file a game owns, before its rows cascade away.
    fn delete_screenshot_files(
        &self,
        conn: &mut SqliteConnection,
        game_id: i64,
    ) -> Result<(), LibraryError> {
        let files: Vec<String> = screenshots::table
            .filter(screenshots::game_id.eq(game_id))
            .select(screenshots::file)
            .load(conn)?;
        for file in files {
            let _ = std::fs::remove_file(self.screenshots_dir.join(&file));
        }
        Ok(())
    }

    /// Delete a game's save-state files (state and thumbnail) before its rows
    /// cascade away.
    fn delete_save_files(
        &self,
        conn: &mut SqliteConnection,
        game_id: i64,
    ) -> Result<(), LibraryError> {
        let rows: Vec<(String, Option<String>)> = save_states::table
            .filter(save_states::game_id.eq(game_id))
            .select((save_states::state_file, save_states::thumb_file))
            .load(conn)?;
        for (state, thumb) in rows {
            let _ = std::fs::remove_file(self.saves_dir.join(&state));
            if let Some(thumb) = thumb {
                let _ = std::fs::remove_file(self.saves_dir.join(&thumb));
            }
        }
        Ok(())
    }

    /// Delete a game's cheat files before its rows cascade away.
    fn delete_cheat_files(
        &self,
        conn: &mut SqliteConnection,
        game_id: i64,
    ) -> Result<(), LibraryError> {
        let files: Vec<String> = cheats::table
            .filter(cheats::game_id.eq(game_id))
            .select(cheats::file)
            .load(conn)?;
        for file in files {
            let _ = std::fs::remove_file(self.cheats_dir.join(&file));
        }
        Ok(())
    }
}

/// Read and write `PRAGMA user_version`.
fn read_version(conn: &mut SqliteConnection) -> Result<i64, LibraryError> {
    let row: UserVersion = diesel::sql_query("PRAGMA user_version").get_result(conn)?;
    Ok(row.user_version)
}

/// Discard whatever is there and create the current schema.
fn reset_schema(conn: &mut SqliteConnection) -> Result<(), LibraryError> {
    conn.batch_execute(
        "PRAGMA foreign_keys = OFF;
         DROP TABLE IF EXISTS game_tags;
         DROP TABLE IF EXISTS tags;
         DROP TABLE IF EXISTS cheats;
         DROP TABLE IF EXISTS save_states;
         DROP TABLE IF EXISTS screenshots;
         DROP TABLE IF EXISTS games;
         PRAGMA foreign_keys = ON;",
    )?;
    conn.batch_execute(SCHEMA_SQL)?;
    conn.batch_execute(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    Ok(())
}

/// A game's id from its stored key.
fn game_id_by_key(conn: &mut SqliteConnection, key: &str) -> Result<Option<i64>, LibraryError> {
    Ok(games::table
        .filter(games::key.eq(key))
        .select(games::id)
        .first(conn)
        .optional()?)
}

/// Every game's tags, keyed by game id, alphabetical and de-duplicated.
fn tags_by_game(conn: &mut SqliteConnection) -> Result<HashMap<i64, Vec<String>>, LibraryError> {
    let rows: Vec<(i64, String)> = game_tags::table
        .inner_join(tags::table)
        .select((game_tags::game_id, tags::name))
        .order(tags::name.asc())
        .load(conn)?;
    let mut map: HashMap<i64, Vec<String>> = HashMap::new();
    for (game_id, name) in rows {
        let list = map.entry(game_id).or_default();
        if !list.contains(&name) {
            list.push(name);
        }
    }
    Ok(map)
}

/// The deepest directory level [`scan_dir`] descends below the folder it is
/// given, so a folder per console (and one more grouping inside it) is found.
pub const MAX_SCAN_DEPTH: usize = 3;

/// Scan `dirs` (and each individually added file) for ROMs this app knows.
///
/// `extra` is the list of individually added files (dragged in or chosen in the
/// dialog); the returned kept list drops entries whose file has vanished.
pub fn collect_games(dirs: &[PathBuf], extra: &[String]) -> (Vec<DiskGame>, Vec<String>) {
    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for dir in dirs {
        for game in scan_dir(dir) {
            if seen.insert(game.path.clone()) {
                games.push(game);
            }
        }
    }
    let mut kept = Vec::new();
    for path in extra {
        if !Path::new(path).is_file() {
            continue;
        }
        let Some(game) = DiskGame::from_path(path) else {
            continue;
        };
        kept.push(path.clone());
        if seen.insert(game.path.clone()) {
            games.push(game);
        }
    }
    (games, kept)
}

/// Parse a save file name into `(rom file, core key, kind, slot, is thumbnail)`.
///
/// The convention is `<rom file name>.<core key>.state<N>` (manual) or
/// `...stateq<N>` (quick), each optionally with a `.png` thumbnail.
fn parse_save_name(name: &str) -> Option<(String, String, SaveKind, u8, bool)> {
    let (base, is_thumb) = match name.strip_suffix(".png") {
        Some(base) => (base, true),
        None => (name, false),
    };
    let mut parts: Vec<&str> = base.split('.').collect();
    let last = parts.pop()?;
    let (kind, digits) = if let Some(digits) = last.strip_prefix("stateq") {
        (SaveKind::Quick, digits)
    } else {
        let digits = last.strip_prefix("state")?;
        (SaveKind::Manual, digits)
    };
    let slot: u8 = digits.parse().ok()?;
    let core_key = parts.pop()?.to_string();
    if parts.is_empty() {
        return None;
    }
    Some((parts.join("."), core_key, kind, slot, is_thumb))
}

/// Scan a `saves/` directory for save-state files, pairing each state with its
/// thumbnail. Only the ROM's file name links a save to a game.
pub fn scan_saves(dir: &Path) -> Vec<DiskSave> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut by_key: HashMap<(String, String, SaveKind, u8), DiskSave> = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((rom_file, core_key, kind, slot, is_thumb)) = parse_save_name(&name) else {
            continue;
        };
        let modified_ms = std::fs::metadata(&path)
            .map(|meta| mtime_millis(&meta))
            .unwrap_or(0);
        let row = by_key
            .entry((rom_file.clone(), core_key.clone(), kind, slot))
            .or_insert_with(|| DiskSave {
                rom_file,
                core_key,
                kind,
                slot,
                state_file: String::new(),
                thumb_file: None,
                modified_ms,
            });
        if is_thumb {
            row.thumb_file = Some(name);
        } else {
            row.state_file = name;
            row.modified_ms = modified_ms;
        }
    }
    by_key
        .into_values()
        .filter(|save| !save.state_file.is_empty())
        .collect()
}

/// Scan a `cheats/` directory for `.cht` files (named after the game's stem).
pub fn scan_cheats(dir: &Path) -> Vec<DiskCheat> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".cht") {
            found.push(DiskCheat {
                stem: stem.to_string(),
                file: name,
            });
        }
    }
    found
}

/// Recursively collect every ROM under `dir` this app knows.
///
/// Descends at most [`MAX_SCAN_DEPTH`] levels, so a folder per console (and one
/// more grouping inside it) is found. The scanner is intentionally forgiving:
/// unreadable subdirectories are skipped, not fatal.
pub fn scan_dir(dir: impl AsRef<Path>) -> Vec<DiskGame> {
    let mut found = Vec::new();
    let mut stack = vec![(PathBuf::from(dir.as_ref()), 0usize)];
    while let Some((current, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_SCAN_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if let Some(game) = DiskGame::from_path(&path) {
                found.push(game);
            }
        }
    }
    found
}

/// A display name from a file name: the stem, underscores turned to spaces.
fn stem_of(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string());
    stem.replace('_', " ")
}

/// The stored form of a ROM path relative to `root` (the library folder):
/// relative when the file is inside it, otherwise the path unchanged. See
/// [`Library::key`].
fn key_for(root: &Path, path: &str) -> String {
    match Path::new(path).strip_prefix(root) {
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => path.to_string(),
    }
}

/// The absolute ROM path for a stored key.
fn resolve_against(root: &Path, key: &str) -> String {
    if Path::new(key).is_absolute() {
        key.to_string()
    } else {
        root.join(key).to_string_lossy().into_owned()
    }
}

/// Tags trimmed, de-duplicated case-insensitively, and sorted for display.
fn normalise_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut words = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if seen.insert(tag.to_lowercase()) {
            words.push(tag.to_string());
        }
    }
    words.sort_by_key(|tag| tag.to_lowercase());
    words
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// A per-process counter, so two screenshots taken in the same millisecond do
/// not collide on a file name.
fn unique_suffix() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn mtime_millis(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A uniquely named empty directory under the OS temp dir.
    fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cgb-db-{}-{n}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn disk(path: &str) -> DiskGame {
        DiskGame {
            path: path.to_string(),
            file_name: Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string()),
            system: SystemId::Nes,
            size: 1,
            mtime_ms: 0,
        }
    }

    fn open_library(root: &Path) -> Library {
        Library::open(&root.join("library.db")).expect("open library")
    }

    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }

    fn count_rows(library: &Library, sql: &str) -> i64 {
        let mut conn = library.conn.borrow_mut();
        diesel::sql_query(sql)
            .get_result::<CountRow>(&mut *conn)
            .unwrap()
            .n
    }

    fn user_version(library: &Library) -> i64 {
        let mut conn = library.conn.borrow_mut();
        let row: UserVersion = diesel::sql_query("PRAGMA user_version")
            .get_result(&mut *conn)
            .unwrap();
        row.user_version
    }

    #[test]
    fn scan_finds_roms_up_to_three_levels_deep() {
        let root = temp_dir("depth");
        std::fs::write(root.join("top.nes"), b"x").unwrap();
        std::fs::create_dir_all(root.join("nes")).unwrap();
        std::fs::write(root.join("nes/a.nes"), b"x").unwrap();
        std::fs::create_dir_all(root.join("nes/han")).unwrap();
        std::fs::write(root.join("nes/han/b.nes"), b"x").unwrap();
        std::fs::create_dir_all(root.join("nes/han/deep")).unwrap();
        std::fs::write(root.join("nes/han/deep/c.nes"), b"x").unwrap();
        std::fs::create_dir_all(root.join("nes/han/deep/deeper")).unwrap();
        std::fs::write(root.join("nes/han/deep/deeper/d.nes"), b"x").unwrap();
        std::fs::write(root.join("nes/readme.txt"), b"x").unwrap();

        let mut stems: Vec<String> = scan_dir(&root)
            .into_iter()
            .map(|game| stem_of(&game.file_name))
            .collect();
        stems.sort();
        assert_eq!(stems, ["a", "b", "c", "top"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn collect_games_merges_extras_and_prunes_missing() {
        let root = temp_dir("collect");
        std::fs::write(root.join("scanned.nes"), b"x").unwrap();
        let extra = root.join("dropped.gba");
        std::fs::write(&extra, b"x").unwrap();
        let extra = extra.to_string_lossy().into_owned();
        let missing = root.join("gone.nes").to_string_lossy().into_owned();

        let (games, kept) = collect_games(std::slice::from_ref(&root), &[extra.clone(), missing]);
        let mut stems: Vec<String> = games
            .into_iter()
            .map(|game| stem_of(&game.file_name))
            .collect();
        stems.sort();
        assert_eq!(stems, ["dropped", "scanned"]);
        assert_eq!(kept, [extra]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_removes_rows_that_are_no_longer_on_disk() {
        let root = temp_dir("sync");
        let library = open_library(&root);
        library.sync(&[disk("/a.nes"), disk("/b.nes")]).unwrap();
        assert_eq!(library.games().unwrap().len(), 2);

        let removed = library.sync(&[disk("/b.nes"), disk("/c.nes")]).unwrap();
        assert_eq!(removed, 1);
        let paths: Vec<String> = library
            .games()
            .unwrap()
            .into_iter()
            .map(|game| game.path)
            .collect();
        assert_eq!(paths, ["/b.nes", "/c.nes"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_are_stored_relative_and_returned_absolute() {
        let root = temp_dir("relative");
        std::fs::create_dir_all(root.join("roms")).unwrap();
        std::fs::write(root.join("roms/mario.nes"), b"x").unwrap();
        let library = open_library(&root);
        library.sync(&scan_dir(root.join("roms"))).unwrap();

        // Stored relative to the library root, so the folder is portable...
        assert!(library.paths().unwrap().contains("roms/mario.nes"));
        // ...but handed back absolute for the app to open.
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.path, root.join("roms/mario.nes").to_string_lossy());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_scan_keeps_a_row_whose_directory_is_not_here() {
        let root = temp_dir("absent");
        let library = open_library(&root);
        // A row written elsewhere: its directory does not exist on this host
        // (another machine, an unmounted volume, another OS's path).
        let foreign = format!("{}/not-mounted/foreign.nes", root.display());
        library.sync(&[disk(&foreign)]).unwrap();
        assert_eq!(library.games().unwrap().len(), 1);

        // A scan that finds nothing here must not wipe it (or its screenshots).
        let removed = library.sync(&[]).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(library.games().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_scan_still_removes_a_file_that_was_deleted() {
        let root = temp_dir("deleted");
        std::fs::create_dir_all(root.join("roms")).unwrap();
        std::fs::write(root.join("roms/gone.nes"), b"x").unwrap();
        let library = open_library(&root);
        library.sync(&scan_dir(root.join("roms"))).unwrap();
        assert_eq!(library.games().unwrap().len(), 1);

        // The directory is still there and the file is gone: a real deletion.
        std::fs::remove_file(root.join("roms/gone.nes")).unwrap();
        let removed = library.sync(&scan_dir(root.join("roms"))).unwrap();
        assert_eq!(removed, 1);
        assert!(library.games().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_keeps_metadata_a_rescan_cannot_know() {
        let root = temp_dir("preserve");
        let library = open_library(&root);
        library.sync(&[disk("/mario.nes")]).unwrap();

        library.rename("/mario.nes", "超级马里奥").unwrap();
        library.set_pinned("/mario.nes", true).unwrap();

        // The file changes on disk; the rescan updates only the file facts.
        let mut changed = disk("/mario.nes");
        changed.size = 4096;
        changed.mtime_ms = 42;
        library.sync(&[changed]).unwrap();

        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.name, "超级马里奥", "the display name survives");
        assert!(game.pinned, "the pin survives");
        assert_eq!(game.size, 4096, "the size is refreshed");
        assert_eq!(game.mtime_ms, 42, "the mtime is refreshed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn play_statistics_accumulate() {
        let root = temp_dir("play");
        let library = open_library(&root);
        library.sync(&[disk("/mario.nes")]).unwrap();

        library
            .note_played("/mario.nes", 1_700_000_000_000)
            .unwrap();
        library
            .note_played("/mario.nes", 1_700_000_001_000)
            .unwrap();
        library.note_playtime("/mario.nes", 90).unwrap();
        library.note_playtime("/mario.nes", 30).unwrap();
        // A game that is not in the library is ignored, not an error.
        library.note_playtime("/elsewhere.nes", 10).unwrap();

        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.play_count, 2);
        assert_eq!(game.play_seconds, 120);
        assert_eq!(game.last_played_at, 1_700_000_001_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn set_system_and_core_survive_a_rescan() {
        let root = temp_dir("overrides");
        let library = open_library(&root);
        library.sync(&[disk("/disc.chd")]).unwrap();
        library
            .set_system("/disc.chd", SystemId::PlayStation)
            .unwrap();
        library
            .set_core("/disc.chd", Some("mednafen_psx_hw"))
            .unwrap();

        library.sync(&[disk("/disc.chd")]).unwrap();
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.system, SystemId::PlayStation);
        assert_eq!(game.core.as_deref(), Some("mednafen_psx_hw"));

        library.set_core("/disc.chd", None).unwrap();
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert!(game.core.is_none(), "None clears the override");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tags_are_shared_and_pruned() {
        let root = temp_dir("tags");
        let library = open_library(&root);
        library.sync(&[disk("/a.nes"), disk("/b.nes")]).unwrap();

        library
            .set_tags("/a.nes", &["RPG".into(), "rpg".into(), " Action ".into()])
            .unwrap();
        library.set_tags("/b.nes", &["rpg".into()]).unwrap();

        let games = library.games().unwrap();
        let tags_of = |path: &str| {
            games
                .iter()
                .find(|game| game.path == path)
                .unwrap()
                .tags
                .clone()
        };
        // Sorted, and "rpg"/"RPG" are one word keeping the first spelling.
        assert_eq!(tags_of("/a.nes"), ["Action", "RPG"]);
        assert_eq!(tags_of("/b.nes"), ["RPG"]);

        library.set_tags("/a.nes", &[]).unwrap();
        library.set_tags("/b.nes", &[]).unwrap();
        assert_eq!(
            count_rows(&library, "SELECT COUNT(*) AS n FROM tags"),
            0,
            "a word nobody points at is pruned"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_screenshot_is_stored_and_the_first_becomes_the_cover() {
        let root = temp_dir("shots");
        let library = open_library(&root);
        library.sync(&[disk("/mario.nes")]).unwrap();
        let png = crate::library::png_codec::encode_png(
            2,
            2,
            &[0, 0, 0, 255, 1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255],
        )
        .unwrap();

        let first = library
            .save_screenshot("/mario.nes", &png, 2, 2, false)
            .unwrap()
            .unwrap();
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.cover, Some(first.id), "the first shot is the cover");
        assert_eq!(game.screenshots, 1);
        assert!(library.screenshots_dir().join(&first.file).is_file());

        // A second shot is not the cover unless forced.
        let second = library
            .save_screenshot("/mario.nes", &png, 2, 2, false)
            .unwrap()
            .unwrap();
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.cover, Some(first.id));
        assert_eq!(game.screenshots, 2);

        // Force the second, delete it: the first becomes the cover again.
        library.set_cover(second.id).unwrap();
        library.remove_screenshot(second.id).unwrap();
        assert!(!library.screenshots_dir().join(&second.file).exists());
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert!(game.cover.is_none());

        // Deleting the game takes its screenshots (rows and files).
        library.remove("/mario.nes").unwrap();
        assert!(!library.screenshots_dir().join(&first.file).exists());
        assert_eq!(library.screenshots().unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_fresh_database_is_at_the_current_version() {
        let root = temp_dir("fresh");
        let library = open_library(&root);
        assert_eq!(user_version(&library), SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_version_mismatch_wipes_and_recreates_the_library() {
        let root = temp_dir("wipe");
        std::fs::create_dir_all(root.join("roms")).unwrap();
        std::fs::write(root.join("roms/mario.nes"), b"x").unwrap();
        let path = root.join("library.db");
        // A foreign database at the wrong version, with a junk row.
        {
            let mut conn = SqliteConnection::establish(&path.to_string_lossy()).unwrap();
            conn.batch_execute(
                "CREATE TABLE games (id INTEGER PRIMARY KEY, path TEXT);
                 INSERT INTO games (path) VALUES ('/junk.nes');
                 PRAGMA user_version = 99;",
            )
            .unwrap();
        }

        // Opening it discards the old contents and creates the current schema.
        let library = Library::open(&path).unwrap();
        assert!(library.games().unwrap().is_empty());
        library.sync(&scan_dir(root.join("roms"))).unwrap();
        assert_eq!(library.games().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_saves_pairs_states_with_thumbnails() {
        let root = temp_dir("saves");
        let saves = root.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("Super Mario Bros. 3.nes.mesen.state1"), b"x").unwrap();
        std::fs::write(saves.join("Super Mario Bros. 3.nes.mesen.state1.png"), b"x").unwrap();
        std::fs::write(saves.join("mario.nes.mgba.stateq0"), b"x").unwrap();

        let found = scan_saves(&saves);
        assert_eq!(found.len(), 2);
        let manual = found.iter().find(|s| s.kind == SaveKind::Manual).unwrap();
        assert_eq!(manual.rom_file, "Super Mario Bros. 3.nes");
        assert_eq!(manual.core_key, "mesen");
        assert_eq!(manual.slot, 1);
        assert_eq!(
            manual.thumb_file.as_deref(),
            Some("Super Mario Bros. 3.nes.mesen.state1.png")
        );
        let quick = found.iter().find(|s| s.kind == SaveKind::Quick).unwrap();
        assert_eq!(quick.rom_file, "mario.nes");
        assert_eq!(quick.slot, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_saves_and_cheats_link_to_games_and_delete_their_files() {
        let root = temp_dir("saves-link");
        let library = open_library(&root);
        library.sync(&[disk("/mario.nes")]).unwrap();
        std::fs::create_dir_all(root.join("saves")).unwrap();
        std::fs::write(root.join("saves/mario.nes.mesen.state1"), b"x").unwrap();
        std::fs::write(root.join("saves/mario.nes.mesen.state1.png"), b"x").unwrap();
        std::fs::create_dir_all(root.join("cheats")).unwrap();
        std::fs::write(root.join("cheats/mario.cht"), b"x").unwrap();

        library
            .sync_saves(&scan_saves(&root.join("saves")))
            .unwrap();
        library
            .sync_cheats(&scan_cheats(&root.join("cheats")))
            .unwrap();
        assert_eq!(
            count_rows(&library, "SELECT COUNT(*) AS n FROM save_states"),
            1
        );
        assert_eq!(count_rows(&library, "SELECT COUNT(*) AS n FROM cheats"), 1);

        // Deleting the game removes its save and cheat files too.
        library.remove("/mario.nes").unwrap();
        assert!(!root.join("saves/mario.nes.mesen.state1").exists());
        assert!(!root.join("saves/mario.nes.mesen.state1.png").exists());
        assert!(!root.join("cheats/mario.cht").exists());
        assert_eq!(
            count_rows(&library, "SELECT COUNT(*) AS n FROM save_states"),
            0
        );
        assert_eq!(count_rows(&library, "SELECT COUNT(*) AS n FROM cheats"), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
