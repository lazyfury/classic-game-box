//! Save states and battery saves on disk.
//!
//! Save-state bytes are opaque: they come straight from `retro_serialize` and
//! go straight back to `retro_unserialize`, so nothing here inspects them.
//! Battery saves (`RETRO_MEMORY_SAVE_RAM`) are written next to them as `.srm`.

use std::path::Path;

use crate::error::LibraryError;

/// Read a save file, or `None` if it does not exist / cannot be read.
pub fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

/// Write a save file, creating the parent directory if needed.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Delete a save file, ignoring "not found".
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Whether a save file exists.
pub fn exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A uniquely named path under the OS temp dir, so tests never collide.
    fn temp_path(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("cgb-test-{}-{n}-{name}", std::process::id()))
    }

    #[test]
    fn a_save_round_trips_and_removes() {
        let path = temp_path("save.srm");
        assert!(!exists(&path));
        write(&path, b"abc").expect("write");
        assert!(exists(&path));
        assert_eq!(read(&path).as_deref(), Some(&b"abc"[..]));
        remove(&path);
        assert!(!exists(&path));
    }

    #[test]
    fn reading_a_missing_file_is_none() {
        assert!(read(&temp_path("missing.srm")).is_none());
    }
}
