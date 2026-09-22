//! The game library: a SQLite table of ROMs plus a folder scanner.
//!
//! The folder is still the source of truth (the old front end's rule): the
//! database is a cache of titles, sizes and play counts, and re-scanning
//! reconciles it against what is on disk. Here that is kept deliberately
//! simple — upsert on scan, list for the UI.

use std::path::{Path, PathBuf};

use cgb_systems::{extension_of, system_for_path, SystemId};
use rusqlite::{params, Connection};

use crate::error::LibraryError;

/// One ROM in the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    /// Absolute path; the primary key.
    pub path: String,
    /// Display title (the file stem by default).
    pub title: String,
    /// Which console the extension says it is for.
    pub system: SystemId,
    /// File size in bytes.
    pub size: u64,
}

impl Game {
    /// A library entry for a ROM path, or `None` if its extension is not a
    /// console this app knows.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        let extension = extension_of(&path.to_string_lossy());
        let known = cgb_systems::SYSTEMS
            .iter()
            .any(|system| system.extensions().contains(&extension.as_str()));
        if !known {
            return None;
        }
        let title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".to_string());
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        Some(Self {
            path: path.to_string_lossy().into_owned(),
            title,
            system: system_for_path(&path.to_string_lossy()),
            size,
        })
    }
}

/// The SQLite-backed library.
pub struct Library {
    conn: Connection,
}

impl Library {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let library = Self { conn };
        library.migrate()?;
        Ok(library)
    }

    fn migrate(&self) -> Result<(), LibraryError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                path        TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                system      TEXT NOT NULL,
                size        INTEGER NOT NULL,
                added_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                last_played INTEGER,
                play_seconds INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS games_order ON games (title COLLATE NOCASE);",
        )?;
        Ok(())
    }

    /// Insert a ROM, or update its title/size if it is already known.
    pub fn upsert(&self, game: &Game) -> Result<(), LibraryError> {
        self.conn.execute(
            "INSERT INTO games (path, title, system, size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
                title = excluded.title,
                system = excluded.system,
                size = excluded.size",
            params![game.path, game.title, game.system.key(), game.size as i64],
        )?;
        Ok(())
    }

    /// Every known game, by title.
    pub fn games(&self) -> Result<Vec<Game>, LibraryError> {
        let mut statement = self
            .conn
            .prepare("SELECT path, title, system, size FROM games ORDER BY title COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| {
            let system: String = row.get(2)?;
            let size: i64 = row.get(3)?;
            Ok(Game {
                path: row.get(0)?,
                title: row.get(1)?,
                system: SystemId::from_key(&system),
                size: size.max(0) as u64,
            })
        })?;
        let mut games = Vec::new();
        for row in rows {
            games.push(row?);
        }
        Ok(games)
    }

    /// Forget a ROM (used when a file is gone from disk).
    pub fn remove(&self, path: &str) -> Result<(), LibraryError> {
        self.conn
            .execute("DELETE FROM games WHERE path = ?1", params![path])?;
        Ok(())
    }
}

/// Recursively collect every ROM under `dir` this app knows.
///
/// The scanner is intentionally forgiving: unreadable subdirectories are
/// skipped, not fatal.
pub fn scan_dir(dir: impl AsRef<Path>) -> Vec<Game> {
    let mut found = Vec::new();
    let mut stack = vec![PathBuf::from(dir.as_ref())];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(game) = Game::from_path(&path) {
                found.push(game);
            }
        }
    }
    found
}
