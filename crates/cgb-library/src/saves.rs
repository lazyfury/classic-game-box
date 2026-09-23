//! Save states and battery saves on disk.
//!
//! Save-state bytes are opaque: they come straight from `retro_serialize` and
//! go straight back to `retro_unserialize`, so nothing here inspects them.
//! Battery saves (`RETRO_MEMORY_SAVE_RAM`) are written next to them as `.srm`.

use std::path::Path;

use crate::error::LibraryError;
use crate::paths::{save_state_path, save_state_thumb_path};

/// How many save-state slots a game has (slot 0 is the quick slot).
pub const SLOT_COUNT: u8 = 10;

/// One save-state slot, as the saves list sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSlot {
    pub slot: u8,
    pub exists: bool,
    /// When the state was written, in epoch milliseconds (0 = never).
    pub modified_ms: i64,
    pub thumbnail: bool,
}

/// List a game's slots for one core: whether each exists, when it was written,
/// and whether it has a thumbnail. Only this core's states are listed, because
/// a state is not portable between cores.
pub fn list_slots(saves: &Path, rom: &Path, core_key: &str) -> Vec<StateSlot> {
    (0..SLOT_COUNT)
        .map(|slot| {
            let path = save_state_path(saves, rom, core_key, slot);
            let exists = path.is_file();
            let modified_ms = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);
            let thumbnail = save_state_thumb_path(saves, rom, core_key, slot).is_file();
            StateSlot {
                slot,
                exists,
                modified_ms,
                thumbnail,
            }
        })
        .collect()
}

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

    #[test]
    fn list_slots_reports_existing_states_and_thumbnails() {
        let dir = temp_path("slots");
        std::fs::create_dir_all(&dir).unwrap();
        let rom = Path::new("/roms/mario.nes");
        std::fs::write(dir.join("mario.nes.mesen.state3"), b"x").unwrap();
        std::fs::write(dir.join("mario.nes.mesen.state3.png"), b"p").unwrap();
        // Another core's state must not show up.
        std::fs::write(dir.join("mario.nes.fbneo.state5"), b"x").unwrap();

        let slots = list_slots(&dir, rom, "mesen");
        assert_eq!(slots.len(), SLOT_COUNT as usize);
        let three = slots.iter().find(|slot| slot.slot == 3).unwrap();
        assert!(three.exists && three.thumbnail);
        assert!(!slots.iter().find(|slot| slot.slot == 2).unwrap().exists);
        assert!(!slots.iter().find(|slot| slot.slot == 5).unwrap().exists);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
