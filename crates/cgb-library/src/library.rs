//! The game library: a SQLite model of the ROM folders, plus a folder scanner.
//!
//! The folder is still the truth about what exists — every refresh rescans it —
//! but the database is now the model, not a cache: it holds the display name,
//! the pin, the play count and time, the screenshots and the cover, none of
//! which a directory listing can express. A rescan reconciles the two: files
//! that appeared are inserted, files that vanished are dropped, and a file that
//! changed keeps every field the player set.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use cgb_systems::{extension_of, system_for_path, SystemId};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::LibraryError;

/// The schema this build understands, stored in `PRAGMA user_version`. A
/// database with a higher number was written by a newer build.
const SCHEMA_VERSION: i64 = 3;

/// One ROM found on disk. What a scan produces; the database reconciles it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskGame {
    /// Absolute path; the reconciliation key.
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
        let known = cgb_systems::SYSTEMS
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
    /// Absolute ROM path; the reconciliation key.
    pub path: String,
    /// The actual file name (`mario.nes`), never edited by a rename.
    pub file_name: String,
    /// The display name, initialised from the file stem and editable.
    pub name: String,
    pub system: SystemId,
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

/// The SQLite-backed library.
pub struct Library {
    conn: Connection,
    /// Where screenshot files live; a sibling of the database.
    screenshots_dir: PathBuf,
}

impl Library {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let screenshots_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("screenshots");
        let library = Self {
            conn,
            screenshots_dir,
        };
        library.migrate()?;
        Ok(library)
    }

    // -- schema -------------------------------------------------------------

    fn migrate(&self) -> Result<(), LibraryError> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            self.migrate_games()?;
        }
        if version < 2 {
            self.migrate_screenshots()?;
        }
        if version < 3 {
            self.migrate_tags()?;
        }
        self.conn
            .execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"), [])?;
        Ok(())
    }

    /// v1: the games and their metadata.
    ///
    /// A fresh database gets the full shape. A version-0 database has the old
    /// `path`-primary-key table with no room for a pin or a play count, so it
    /// is rebuilt and its rows carried across; the display name starts as the
    /// old `title`, the file name is backfilled from the path.
    fn migrate_games(&self) -> Result<(), LibraryError> {
        if !self.has_table("games")? {
            self.conn.execute_batch(GAMES_SCHEMA)?;
        } else if !self.has_column("games", "id")? {
            self.conn.execute_batch(
                "ALTER TABLE games RENAME TO games_old;
                 CREATE TABLE games (
                    id             INTEGER PRIMARY KEY,
                    path           TEXT NOT NULL UNIQUE,
                    file_name      TEXT NOT NULL DEFAULT '',
                    name           TEXT NOT NULL,
                    system         TEXT NOT NULL,
                    size           INTEGER NOT NULL,
                    mtime_ms       INTEGER NOT NULL DEFAULT 0,
                    added_at       INTEGER NOT NULL,
                    last_played_at INTEGER NOT NULL DEFAULT 0,
                    play_count     INTEGER NOT NULL DEFAULT 0,
                    play_seconds   INTEGER NOT NULL DEFAULT 0,
                    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1))
                 );
                 INSERT INTO games (path, file_name, name, system, size, mtime_ms,
                                    added_at, last_played_at, play_count, play_seconds, pinned)
                    SELECT path, '', title, system, size, 0,
                           added_at, COALESCE(last_played, 0), 0, play_seconds, 0
                      FROM games_old;
                 DROP TABLE games_old;",
            )?;
            self.backfill_file_names()?;
        }
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS games_order
                ON games (pinned DESC, last_played_at DESC, name COLLATE NOCASE)",
            [],
        )?;
        Ok(())
    }

    /// v2: the screenshots, and the cover that points at one of them.
    fn migrate_screenshots(&self) -> Result<(), LibraryError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS screenshots (
                id         INTEGER PRIMARY KEY,
                game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                file       TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                width      INTEGER NOT NULL DEFAULT 0,
                height     INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS screenshots_by_game
                ON screenshots (game_id, created_at DESC);",
        )?;
        if !self.has_column("games", "cover_id")? {
            self.conn.execute(
                "ALTER TABLE games ADD COLUMN cover_id INTEGER
                    REFERENCES screenshots(id) ON DELETE SET NULL",
                [],
            )?;
        }
        Ok(())
    }

    /// v3: the tags.
    fn migrate_tags(&self) -> Result<(), LibraryError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tags (
                id   INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE
             );
             CREATE TABLE IF NOT EXISTS game_tags (
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                tag_id  INTEGER NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
                PRIMARY KEY (game_id, tag_id)
             );
             CREATE INDEX IF NOT EXISTS game_tags_by_tag ON game_tags (tag_id);",
        )?;
        Ok(())
    }

    /// The old table's rows have no file name; take it from the path.
    fn backfill_file_names(&self) -> Result<(), LibraryError> {
        let mut statement = self
            .conn
            .prepare("SELECT path FROM games WHERE file_name = ''")?;
        let paths: Vec<String> = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        drop(statement);
        for path in paths {
            let file_name = Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            self.conn.execute(
                "UPDATE games SET file_name = ?2 WHERE path = ?1",
                params![path, file_name],
            )?;
        }
        Ok(())
    }

    fn has_table(&self, table: &str) -> Result<bool, LibraryError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool, LibraryError> {
        let mut statement = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // -- reconciliation -----------------------------------------------------

    /// Reconcile the database with a fresh scan.
    ///
    /// A path already known keeps its name, pin, play statistics, screenshots
    /// and cover; only the file name, size and modification time are refreshed.
    /// A path that appeared is inserted with its display name taken from the
    /// file stem. A row whose file is gone is deleted. Returns how many rows
    /// were removed.
    pub fn sync(&self, found: &[DiskGame]) -> Result<usize, LibraryError> {
        let known = self.paths()?;
        let now = now_millis();
        for game in found {
            if known.contains(game.path.as_str()) {
                self.conn.execute(
                    "UPDATE games SET file_name = ?2, size = ?3, mtime_ms = ?4 WHERE path = ?1",
                    params![game.path, game.file_name, game.size as i64, game.mtime_ms],
                )?;
            } else {
                self.conn.execute(
                    "INSERT INTO games (path, file_name, name, system, size, mtime_ms, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        game.path,
                        game.file_name,
                        stem_of(&game.file_name),
                        game.system.key(),
                        game.size as i64,
                        game.mtime_ms,
                        now,
                    ],
                )?;
            }
        }
        let keep: HashSet<&str> = found.iter().map(|game| game.path.as_str()).collect();
        let mut removed = 0;
        for path in self.paths()? {
            if !keep.contains(path.as_str()) {
                self.remove(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Every known game, ordered the way the library shows them: pinned first,
    /// then most recently played, then by name.
    pub fn games(&self) -> Result<Vec<Game>, LibraryError> {
        let mut statement = self.conn.prepare(
            "SELECT g.id, g.path, g.file_name, g.name, g.system, g.size, g.mtime_ms,
                    g.added_at, g.last_played_at, g.play_count, g.play_seconds, g.pinned,
                    g.cover_id,
                    (SELECT COUNT(*) FROM screenshots s WHERE s.game_id = g.id)
             FROM games g
             ORDER BY g.pinned DESC, g.last_played_at DESC, g.name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            let system: String = row.get(4)?;
            Ok(Game {
                id: row.get(0)?,
                path: row.get(1)?,
                file_name: row.get(2)?,
                name: row.get(3)?,
                system: SystemId::from_key(&system),
                size: row.get::<_, i64>(5)?.max(0) as u64,
                mtime_ms: row.get(6)?,
                added_at: row.get(7)?,
                last_played_at: row.get(8)?,
                play_count: row.get(9)?,
                play_seconds: row.get(10)?,
                pinned: row.get::<_, i64>(11)? != 0,
                cover: row.get(12)?,
                screenshots: row.get(13)?,
                tags: Vec::new(),
            })
        })?;
        let mut games = Vec::new();
        for row in rows {
            games.push(row?);
        }
        let mut tags = self.tags_by_game()?;
        for game in &mut games {
            if let Some(list) = tags.remove(&game.id) {
                game.tags = list;
            }
        }
        Ok(games)
    }

    /// Every game's tags, keyed by game id, alphabetical and de-duplicated.
    fn tags_by_game(&self) -> Result<HashMap<i64, Vec<String>>, LibraryError> {
        let mut statement = self.conn.prepare(
            "SELECT gt.game_id, t.name
               FROM game_tags gt JOIN tags t ON t.id = gt.tag_id
              ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (id, name) = row?;
            tags.entry(id).or_default().push(name);
        }
        Ok(tags)
    }

    /// Replace a game's tags with exactly these words.
    ///
    /// A word is shared if it already exists (case-insensitively) and created
    /// otherwise; words nothing points at are pruned, so the table cannot grow
    /// rows the UI can never show.
    pub fn set_tags(&self, path: &str, tags: &[String]) -> Result<(), LibraryError> {
        let Some(id) = self.game_id(path)? else {
            return Ok(());
        };
        let wanted = normalise_tags(tags);
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("DELETE FROM game_tags WHERE game_id = ?1", params![id])?;
        for name in &wanted {
            transaction.execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![name],
            )?;
            let tag_id: i64 = transaction.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO game_tags (game_id, tag_id) VALUES (?1, ?2)",
                params![id, tag_id],
            )?;
        }
        transaction.commit()?;
        self.prune_tags()?;
        Ok(())
    }

    fn game_id(&self, path: &str) -> Result<Option<i64>, LibraryError> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM games WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    fn prune_tags(&self) -> Result<(), LibraryError> {
        self.conn.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM game_tags)",
            [],
        )?;
        Ok(())
    }

    /// Pin a game to the top of the library, or unpin it.
    pub fn set_pinned(&self, path: &str, pinned: bool) -> Result<(), LibraryError> {
        self.conn.execute(
            "UPDATE games SET pinned = ?2 WHERE path = ?1",
            params![path, pinned as i64],
        )?;
        Ok(())
    }

    /// Record that a game was run: bump its count and stamp the time. Does
    /// nothing for a path that is not in the library (`--rom` can name any
    /// file on the machine).
    pub fn note_played(&self, path: &str, at: i64) -> Result<(), LibraryError> {
        self.conn.execute(
            "UPDATE games SET last_played_at = ?2, play_count = play_count + 1 WHERE path = ?1",
            params![path, at],
        )?;
        Ok(())
    }

    /// Add whole seconds to a game's total play time. A non-positive or absent
    /// game is ignored.
    pub fn note_playtime(&self, path: &str, seconds: i64) -> Result<(), LibraryError> {
        if seconds <= 0 {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE games SET play_seconds = play_seconds + ?2 WHERE path = ?1",
            params![path, seconds],
        )?;
        Ok(())
    }

    /// Rename a game's display name. The file on disk is untouched.
    pub fn rename(&self, path: &str, name: &str) -> Result<(), LibraryError> {
        self.conn.execute(
            "UPDATE games SET name = ?2 WHERE path = ?1",
            params![path, name],
        )?;
        Ok(())
    }

    /// Forget a ROM: its screenshots (rows and files) go with it.
    pub fn remove(&self, path: &str) -> Result<(), LibraryError> {
        if let Some(id) = self.game_id(path)? {
            self.delete_screenshot_files(id)?;
        }
        self.conn
            .execute("DELETE FROM games WHERE path = ?1", params![path])?;
        Ok(())
    }

    // -- screenshots --------------------------------------------------------

    /// The directory screenshot files live in.
    pub fn screenshots_dir(&self) -> &Path {
        &self.screenshots_dir
    }

    /// Every screenshot, newest first.
    pub fn screenshots(&self) -> Result<Vec<Screenshot>, LibraryError> {
        let mut statement = self.conn.prepare(
            "SELECT id, game_id, file, created_at, width, height
               FROM screenshots
              ORDER BY created_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Screenshot {
                id: row.get(0)?,
                game_id: row.get(1)?,
                file: row.get(2)?,
                created_at: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
            })
        })?;
        let mut shots = Vec::new();
        for row in rows {
            shots.push(row?);
        }
        Ok(shots)
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
        let Some(game_id) = self.game_id(game_path)? else {
            return Ok(None);
        };
        std::fs::create_dir_all(&self.screenshots_dir)?;
        let file = format!("{}-{:08x}.png", now_millis(), unique_suffix());
        std::fs::write(self.screenshots_dir.join(&file), png)?;

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM screenshots WHERE game_id = ?1",
            params![game_id],
            |row| row.get(0),
        )?;
        let cover = as_cover || count == 0;
        let created_at = now_millis();
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO screenshots (game_id, file, created_at, width, height)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![game_id, file, created_at, width, height],
        )?;
        let id = transaction.last_insert_rowid();
        if cover {
            transaction.execute(
                "UPDATE games SET cover_id = ?2 WHERE id = ?1",
                params![game_id, id],
            )?;
        }
        transaction.commit()?;
        Ok(Some(Screenshot {
            id,
            game_id,
            file,
            created_at,
            width,
            height,
        }))
    }

    /// Make a screenshot the cover of its game.
    pub fn set_cover(&self, id: i64) -> Result<bool, LibraryError> {
        let game_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT game_id FROM screenshots WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(game_id) = game_id else {
            return Ok(false);
        };
        self.conn.execute(
            "UPDATE games SET cover_id = ?2 WHERE id = ?1",
            params![game_id, id],
        )?;
        Ok(true)
    }

    /// Delete one screenshot (row and file). If it was the cover, the newest
    /// remaining picture takes over, or the cover is cleared.
    pub fn remove_screenshot(&self, id: i64) -> Result<bool, LibraryError> {
        let row: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT game_id, file FROM screenshots WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((game_id, file)) = row else {
            return Ok(false);
        };
        // The cover_id foreign key clears itself on delete, so read whether
        // this was the cover *before* deleting the row.
        let cover_id: Option<i64> = self.conn.query_row(
            "SELECT cover_id FROM games WHERE id = ?1",
            params![game_id],
            |row| row.get(0),
        )?;
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("DELETE FROM screenshots WHERE id = ?1", params![id])?;
        if cover_id == Some(id) {
            transaction.execute(
                "UPDATE games SET cover_id = (
                     SELECT id FROM screenshots WHERE game_id = ?1
                      ORDER BY created_at DESC, id DESC LIMIT 1)
                 WHERE id = ?1",
                params![game_id],
            )?;
        }
        transaction.commit()?;
        let _ = std::fs::remove_file(self.screenshots_dir.join(&file));
        Ok(true)
    }

    /// The file name of a screenshot, if the row exists.
    pub fn screenshot_file(&self, id: i64) -> Result<Option<String>, LibraryError> {
        let file = self
            .conn
            .query_row(
                "SELECT file FROM screenshots WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(file)
    }

    /// The absolute path of a screenshot's PNG, if the row exists.
    pub fn screenshot_path(&self, id: i64) -> Result<Option<PathBuf>, LibraryError> {
        Ok(self
            .screenshot_file(id)?
            .map(|file| self.screenshots_dir.join(file)))
    }

    /// Unlink every screenshot file of a game and drop its rows.
    fn delete_screenshot_files(&self, game_id: i64) -> Result<(), LibraryError> {
        let mut statement = self
            .conn
            .prepare("SELECT file FROM screenshots WHERE game_id = ?1")?;
        let files: Vec<String> = statement
            .query_map(params![game_id], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        drop(statement);
        for file in files {
            let _ = std::fs::remove_file(self.screenshots_dir.join(file));
        }
        self.conn.execute(
            "DELETE FROM screenshots WHERE game_id = ?1",
            params![game_id],
        )?;
        Ok(())
    }

    fn paths(&self) -> Result<HashSet<String>, LibraryError> {
        let mut statement = self.conn.prepare("SELECT path FROM games")?;
        let paths = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(paths)
    }
}

/// The `games` table, as a fresh database creates it. `cover_id` arrives in
/// the next migration, so it is not here — an `ALTER` and a `CREATE` cannot
/// both add it.
const GAMES_SCHEMA: &str = "
CREATE TABLE games (
    id             INTEGER PRIMARY KEY,
    path           TEXT NOT NULL UNIQUE,
    file_name      TEXT NOT NULL DEFAULT '',
    name           TEXT NOT NULL,
    system         TEXT NOT NULL,
    size           INTEGER NOT NULL,
    mtime_ms       INTEGER NOT NULL DEFAULT 0,
    added_at       INTEGER NOT NULL,
    last_played_at INTEGER NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    play_seconds   INTEGER NOT NULL DEFAULT 0,
    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1))
);";

/// The display name for a file: its stem, or the whole name if it has none.
fn stem_of(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string())
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

/// The deepest directory level [`scan_dir`] descends below the folder it is
/// given (the folder itself is level 0). Three is enough for a per-console
/// layout with one extra grouping (`nes/汉化/game.nes`) without wandering into
/// a deep tree of assets or screenshots.
pub const MAX_SCAN_DEPTH: usize = 3;

/// The games from scanned folders plus individually added ROM files.
///
/// ROM paths are de-duplicated, so a file that is both inside a scanned folder
/// and listed explicitly appears once. `extra` paths that no longer point at a
/// file are skipped; the ones that survive are returned so the caller can prune
/// its stored list.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cgb-scan-{name}-{}", std::process::id()));
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

    fn open_library(root: &Path) -> Library {
        Library::open(&root.join("library.db")).expect("open library")
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
        let left: i64 = library
            .conn
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0, "a word nobody points at is pruned");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_screenshot_is_stored_and_the_first_becomes_the_cover() {
        let root = temp_dir("shots");
        let library = open_library(&root);
        library.sync(&[disk("/mario.nes")]).unwrap();
        let png = crate::png_codec::encode_png(
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

        // Force the second, delete it: the newest remaining takes over.
        library.set_cover(second.id).unwrap();
        library.remove_screenshot(second.id).unwrap();
        assert!(!library.screenshots_dir().join(&second.file).exists());
        let game = library.games().unwrap().into_iter().next().unwrap();
        assert_eq!(game.cover, Some(first.id));

        // Deleting the game takes its screenshots (rows and files).
        library.remove("/mario.nes").unwrap();
        assert!(!library.screenshots_dir().join(&first.file).exists());
        assert_eq!(library.screenshots().unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_fresh_database_migrates_to_the_current_version() {
        let root = temp_dir("fresh");
        let library = open_library(&root);
        let version: i64 = library
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_old_database_is_upgraded_without_losing_rows() {
        let root = temp_dir("upgrade");
        let path = root.join("library.db");
        // The version-0 shape: no id, `title`, no pin column.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE games (
                    path TEXT PRIMARY KEY, title TEXT NOT NULL, system TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    added_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                    last_played INTEGER, play_seconds INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO games (path, title, system, size) VALUES
                    ('/roms/mario.nes', 'mario', 'nes', 1024);",
            )
            .unwrap();
        }

        let library = Library::open(&path).unwrap();
        let games = library.games().unwrap();
        assert_eq!(games.len(), 1);
        let game = &games[0];
        assert_eq!(game.name, "mario", "the old title becomes the name");
        assert_eq!(game.file_name, "mario.nes", "the file name is backfilled");
        assert_eq!(game.size, 1024);
        assert!(!game.pinned);
        let _ = std::fs::remove_dir_all(&root);
    }
}
