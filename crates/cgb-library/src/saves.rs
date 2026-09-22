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
