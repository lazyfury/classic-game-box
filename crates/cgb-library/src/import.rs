//! Importing ROM files into the library folder.
//!
//! The library is one folder: the ROMs, their metadata and their screenshots.
//! A game that was merely *pointed at* would break the moment its file moved,
//! so adding a game **copies** it in. This mirrors the legacy Electron front
//! end (`legacy/electron/src/main/library.ts::Library.add`).
//!
//! The rules are deliberately conservative:
//!
//! * a source already inside the library folder is skipped — copying a file
//!   onto itself is not what "add" means;
//! * a file that is not a ROM this app knows is skipped;
//! * a taken name gets a ` (2)`, ` (3)`… suffix rather than being overwritten,
//!   because overwriting is how a player loses a game;
//! * a copy that fails is reported, not fatal, so one bad file does not abort
//!   the batch.

use std::path::{Path, PathBuf};

use crate::library::DiskGame;

/// What an [`import_roms`] call did, for the caller to report to the player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Destination paths written into the library folder.
    pub copied: Vec<PathBuf>,
    /// Sources already inside the library folder, left alone.
    pub already_inside: Vec<PathBuf>,
    /// Sources whose extension is not a console this app knows.
    pub unknown: Vec<PathBuf>,
    /// Sources that could not be copied, with the error text.
    pub failed: Vec<(PathBuf, String)>,
}

impl ImportReport {
    /// How many files were written.
    pub fn copied_count(&self) -> usize {
        self.copied.len()
    }

    /// How many sources were skipped (unknown extension or already inside).
    pub fn skipped_count(&self) -> usize {
        self.unknown.len() + self.already_inside.len()
    }

    /// Whether the call touched nothing at all.
    pub fn is_empty(&self) -> bool {
        self.copied.is_empty()
            && self.already_inside.is_empty()
            && self.unknown.is_empty()
            && self.failed.is_empty()
    }
}

/// Copy ROM `sources` into `library_dir`.
///
/// Returns a report of what happened; nothing here is fatal. `library_dir` is
/// created if it does not exist.
pub fn import_roms(library_dir: impl AsRef<Path>, sources: &[PathBuf]) -> ImportReport {
    let library_dir = library_dir.as_ref();
    let mut report = ImportReport::default();
    if !sources.is_empty() {
        let _ = std::fs::create_dir_all(library_dir);
    }
    for source in sources {
        if !source.is_file() {
            report.unknown.push(source.clone());
            continue;
        }
        if is_inside(library_dir, source) {
            report.already_inside.push(source.clone());
            continue;
        }
        if DiskGame::from_path(source).is_none() {
            report.unknown.push(source.clone());
            continue;
        }
        let Some(name) = source.file_name() else {
            report.unknown.push(source.clone());
            continue;
        };
        let target = free_name(library_dir, name);
        match std::fs::copy(source, &target) {
            Ok(_) => report.copied.push(target),
            Err(error) => report.failed.push((source.clone(), error.to_string())),
        }
    }
    report
}

/// `name` if it is free under `dir`, otherwise `stem (2).ext`, `stem (3).ext`…
fn free_name(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let name = name.to_string_lossy();
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (&name[..], ""),
    };
    let mut n = 2u32;
    loop {
        let candidate = dir.join(format!("{stem} ({n}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// True when `candidate` is `root` itself or something inside it.
///
/// Both sides are canonicalized first so `..` is collapsed *before* the
/// comparison and a path like `/library/../secrets` cannot pass.
fn is_inside(root: &Path, candidate: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let candidate = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf());
    candidate == root || candidate.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty directory under the temp dir.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cgb-import-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, bytes).expect("write file");
    }

    #[test]
    fn a_rom_is_copied_into_the_library_and_the_source_survives() {
        let root = temp_dir("copies");
        let library = root.join("library");
        let source = root.join("mario.nes");
        write(&source, b"rom");

        let report = import_roms(&library, std::slice::from_ref(&source));

        assert_eq!(report.copied, vec![library.join("mario.nes")]);
        assert_eq!(report.copied_count(), 1);
        assert!(library.join("mario.nes").is_file(), "copied in");
        assert!(source.is_file(), "the source is left where it was");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_taken_name_is_made_unique_rather_than_overwritten() {
        let root = temp_dir("unique");
        let library = root.join("library");
        write(&library.join("mario.nes"), b"first");
        let source = root.join("mario.nes");
        write(&source, b"second");

        let report = import_roms(&library, std::slice::from_ref(&source));

        assert_eq!(report.copied, vec![library.join("mario (2).nes")]);
        assert_eq!(
            std::fs::read(library.join("mario.nes")).unwrap(),
            b"first",
            "the existing game is untouched"
        );
        assert_eq!(
            std::fs::read(library.join("mario (2).nes")).unwrap(),
            b"second"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_already_inside_the_library_is_skipped() {
        let root = temp_dir("inside");
        let library = root.join("library");
        let source = library.join("mario.nes");
        write(&source, b"rom");

        let report = import_roms(&library, std::slice::from_ref(&source));

        assert!(report.copied.is_empty());
        assert_eq!(report.already_inside, vec![source]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_rom_is_skipped() {
        let root = temp_dir("unknown");
        let library = root.join("library");
        let source = root.join("notes.txt");
        write(&source, b"hello");

        let report = import_roms(&library, std::slice::from_ref(&source));

        assert!(report.copied.is_empty());
        assert_eq!(report.unknown, vec![source]);
        assert!(!library.join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
