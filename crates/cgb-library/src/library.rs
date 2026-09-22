//! The game library: a SQLite table of ROMs plus a folder scanner.
//!
//! The folder is still the source of truth (the old front end's rule): the
//! database is a cache of titles, sizes and play counts, and re-scanning
//! reconciles it against what is on disk. Here that is kept deliberately
//! simple — upsert on scan, list for the UI.

use std::collections::HashSet;
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

    /// Reconcile the database with a fresh scan: upsert everything in `found`
    /// and drop rows whose file is no longer there. Returns how many stale
    /// rows were removed.
    ///
    /// The folder is the source of truth, so this is what a `--rescan` does:
    /// after ROMs are moved or deleted outside the app, the cache is brought
    /// back in line instead of keeping dead paths forever.
    pub fn sync(&self, found: &[Game]) -> Result<usize, LibraryError> {
        for game in found {
            self.upsert(game)?;
        }
        let keep: HashSet<&str> = found.iter().map(|game| game.path.as_str()).collect();
        let mut removed = 0;
        for game in self.games()? {
            if !keep.contains(game.path.as_str()) {
                self.remove(&game.path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// The deepest directory level [`scan_dir`] descends below the folder it is
/// given (the folder itself is level 0). Three is enough for a per-console
/// layout with one extra grouping (`nes/汉化/game.nes`) without wandering into
/// a deep tree of assets or screenshots.
pub const MAX_SCAN_DEPTH: usize = 3;

/// Recursively collect every ROM under `dir` this app knows.
///
/// Descends at most [`MAX_SCAN_DEPTH`] levels, so a folder per console (and one
/// more grouping inside it) is found. The scanner is intentionally forgiving:
/// unreadable subdirectories are skipped, not fatal.
pub fn scan_dir(dir: impl AsRef<Path>) -> Vec<Game> {
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
            } else if let Some(game) = Game::from_path(&path) {
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

    #[test]
    fn scan_finds_roms_up_to_three_levels_deep() {
        let root = temp_dir("depth");
        // no subdir: root/top.nes
        std::fs::write(root.join("top.nes"), b"x").unwrap();
        // one subdir: root/nes/a.nes
        std::fs::create_dir_all(root.join("nes")).unwrap();
        std::fs::write(root.join("nes/a.nes"), b"x").unwrap();
        // two subdirs: root/nes/han/b.nes
        std::fs::create_dir_all(root.join("nes/han")).unwrap();
        std::fs::write(root.join("nes/han/b.nes"), b"x").unwrap();
        // three subdirs: root/nes/han/deep/c.nes — the deepest we scan
        std::fs::create_dir_all(root.join("nes/han/deep")).unwrap();
        std::fs::write(root.join("nes/han/deep/c.nes"), b"x").unwrap();
        // four subdirs: too deep, ignored
        std::fs::create_dir_all(root.join("nes/han/deep/deeper")).unwrap();
        std::fs::write(root.join("nes/han/deep/deeper/d.nes"), b"x").unwrap();
        // not a ROM, ignored
        std::fs::write(root.join("nes/readme.txt"), b"x").unwrap();

        let mut titles: Vec<String> = scan_dir(&root).into_iter().map(|game| game.title).collect();
        titles.sort();
        assert_eq!(titles, ["a", "b", "c", "top"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_removes_rows_that_are_no_longer_on_disk() {
        let root = temp_dir("sync");
        let db = root.join("library.db");
        let library = Library::open(&db).unwrap();
        let game = |path: &str| Game {
            path: path.to_string(),
            title: path.to_string(),
            system: SystemId::Nes,
            size: 1,
        };
        library.sync(&[game("/a.nes"), game("/b.nes")]).unwrap();
        assert_eq!(library.games().unwrap().len(), 2);

        let removed = library.sync(&[game("/b.nes"), game("/c.nes")]).unwrap();
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
}
