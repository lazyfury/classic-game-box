//! Save states and battery saves on disk.
//!
//! Save-state bytes are opaque: they come straight from `retro_serialize` and
//! go straight back to `retro_unserialize`, so nothing here inspects them.
//! Battery saves (`RETRO_MEMORY_SAVE_RAM`) are written next to them as `.srm`.

use std::path::{Path, PathBuf};

use crate::paths::{
    quick_state_path, quick_state_thumb_path, save_state_path, save_state_thumb_path,
};

use super::error::LibraryError;

/// How many fixed manual save-state slots a game has (slots `1..=9`).
pub const MANUAL_SLOT_COUNT: u8 = 9;

/// How many rolling quick saves are kept. The newest is rank `0`; saving again
/// pushes the older ones down and drops the oldest, so a bad save can always be
/// rolled back to the previous one.
pub const QUICK_SLOT_COUNT: u8 = 3;

/// One save-state slot, as the saves list sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSlot {
    pub slot: u8,
    pub exists: bool,
    /// When the state was written, in epoch milliseconds (0 = never).
    pub modified_ms: i64,
    pub thumbnail: bool,
}

/// Describe one slot from its state and thumbnail paths.
fn slot_info(slot: u8, state: PathBuf, thumb: PathBuf) -> StateSlot {
    let exists = state.is_file();
    let modified_ms = std::fs::metadata(&state)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let thumbnail = thumb.is_file();
    StateSlot {
        slot,
        exists,
        modified_ms,
        thumbnail,
    }
}

/// List a game's fixed manual slots for one core: whether each exists, when it
/// was written, and whether it has a thumbnail. Only this core's states are
/// listed, because a state is not portable between cores.
pub fn list_slots(saves: &Path, rom: &Path, core_key: &str) -> Vec<StateSlot> {
    (1..=MANUAL_SLOT_COUNT)
        .map(|slot| {
            slot_info(
                slot,
                save_state_path(saves, rom, core_key, slot),
                save_state_thumb_path(saves, rom, core_key, slot),
            )
        })
        .collect()
}

/// List a game's rolling quick saves for one core, newest first (`0` is the
/// newest rank).
pub fn list_quick_slots(saves: &Path, rom: &Path, core_key: &str) -> Vec<StateSlot> {
    (0..QUICK_SLOT_COUNT)
        .map(|rank| {
            slot_info(
                rank,
                quick_state_path(saves, rom, core_key, rank),
                quick_state_thumb_path(saves, rom, core_key, rank),
            )
        })
        .collect()
}

/// Move a quick save (state and thumbnail together) from one rank to another,
/// replacing whatever sat at the destination.
fn move_quick_at(saves: &Path, rom: &Path, core_key: &str, from: u8, to: u8) {
    let state = quick_state_path(saves, rom, core_key, from);
    if state.is_file() {
        let _ = std::fs::rename(&state, quick_state_path(saves, rom, core_key, to));
    }
    let thumb = quick_state_thumb_path(saves, rom, core_key, from);
    let to_thumb = quick_state_thumb_path(saves, rom, core_key, to);
    let _ = std::fs::remove_file(&to_thumb);
    if thumb.is_file() {
        let _ = std::fs::rename(&thumb, &to_thumb);
    }
}

/// Remove a quick save (state and thumbnail) at `rank`, ignoring missing files.
fn remove_quick_at(saves: &Path, rom: &Path, core_key: &str, rank: u8) {
    let _ = std::fs::remove_file(quick_state_path(saves, rom, core_key, rank));
    let _ = std::fs::remove_file(quick_state_thumb_path(saves, rom, core_key, rank));
}

/// Roll the quick-save stack down one rank to make room for a new newest save:
/// drop the oldest, then shift every remaining save one rank older. The caller
/// writes the new state to rank `0`. A core that cannot serialize must check
/// before calling this, or a save is consumed for nothing.
pub fn roll_quick(saves: &Path, rom: &Path, core_key: &str) {
    let oldest = QUICK_SLOT_COUNT - 1;
    remove_quick_at(saves, rom, core_key, oldest);
    for rank in (0..oldest).rev() {
        move_quick_at(saves, rom, core_key, rank, rank + 1);
    }
}

/// Delete the quick save at `rank` and compact the older ones into the gap, so
/// the stack stays contiguous (newest first) and the next save still has room.
pub fn compact_quick(saves: &Path, rom: &Path, core_key: &str, rank: u8) {
    remove_quick_at(saves, rom, core_key, rank);
    for r in rank..QUICK_SLOT_COUNT - 1 {
        move_quick_at(saves, rom, core_key, r + 1, r);
    }
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

/// Delete the quick-save files the old single-slot layout left behind
/// (`*.state0` and `*.state0.png`). The rolling quick stack uses `stateqN` now,
/// so those files are unreachable; the manual `state1..9` slots are unchanged
/// and are kept.
pub fn remove_legacy_quick(saves: &Path) {
    let Ok(entries) = std::fs::read_dir(saves) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".state0") || name.ends_with(".state0.png") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
        assert_eq!(slots.len(), MANUAL_SLOT_COUNT as usize);
        // Manual slots are 1-based: there is no slot 0 any more.
        assert_eq!(slots.first().unwrap().slot, 1);
        let three = slots.iter().find(|slot| slot.slot == 3).unwrap();
        assert!(three.exists && three.thumbnail);
        assert!(!slots.iter().find(|slot| slot.slot == 2).unwrap().exists);
        assert!(!slots.iter().find(|slot| slot.slot == 5).unwrap().exists);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_quick_slots_reports_the_rolling_stack() {
        let dir = temp_path("quick");
        std::fs::create_dir_all(&dir).unwrap();
        let rom = Path::new("/roms/mario.nes");
        // Rank 0 (newest) and rank 2 exist; rank 1 is an empty gap.
        std::fs::write(dir.join("mario.nes.mesen.stateq0"), b"x").unwrap();
        std::fs::write(dir.join("mario.nes.mesen.stateq2.png"), b"p").unwrap();
        // A manual slot with the same number must not be confused for a quick
        // save (different namespace).
        std::fs::write(dir.join("mario.nes.mesen.state1"), b"x").unwrap();

        let quick = list_quick_slots(&dir, rom, "mesen");
        assert_eq!(quick.len(), QUICK_SLOT_COUNT as usize);
        assert!(quick[0].exists && !quick[0].thumbnail);
        assert!(!quick[1].exists);
        assert!(!quick[2].exists && quick[2].thumbnail);
        // The manual slot 1 is not a quick save.
        assert!(!list_slots(&dir, rom, "mesen")
            .iter()
            .any(|slot| slot.slot == 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mirror `Session::quick_save`: roll the stack, then write the newest save
    /// at rank 0. Returns the bytes now at each rank, newest first.
    fn quick_push(dir: &Path, rom: &Path, core: &str, bytes: &[u8]) -> Vec<Option<Vec<u8>>> {
        roll_quick(dir, rom, core);
        write(&quick_state_path(dir, rom, core, 0), bytes).unwrap();
        (0..QUICK_SLOT_COUNT)
            .map(|rank| read(&quick_state_path(dir, rom, core, rank)))
            .collect()
    }

    #[test]
    fn rolling_quick_keeps_the_newest_and_drops_the_oldest() {
        let dir = temp_path("roll");
        std::fs::create_dir_all(&dir).unwrap();
        let rom = Path::new("/roms/mario.nes");
        let core = "mesen";

        // First save: only rank 0 is written.
        let a = quick_push(&dir, rom, core, b"A");
        assert_eq!(a[0].as_deref(), Some(&b"A"[..]));
        assert!(a[1].is_none() && a[2].is_none());

        // Second: the old save moves to rank 1, the new one takes rank 0.
        let b = quick_push(&dir, rom, core, b"B");
        assert_eq!(b[0].as_deref(), Some(&b"B"[..]));
        assert_eq!(b[1].as_deref(), Some(&b"A"[..]));
        assert!(b[2].is_none());

        // Third: A -> 2, B -> 1, new C at 0.
        let c = quick_push(&dir, rom, core, b"C");
        assert_eq!(c[0].as_deref(), Some(&b"C"[..]));
        assert_eq!(c[1].as_deref(), Some(&b"B"[..]));
        assert_eq!(c[2].as_deref(), Some(&b"A"[..]));

        // Fourth: the oldest (A) is destroyed; D, C, B remain.
        let d = quick_push(&dir, rom, core, b"D");
        assert_eq!(d[0].as_deref(), Some(&b"D"[..]));
        assert_eq!(d[1].as_deref(), Some(&b"C"[..]));
        assert_eq!(d[2].as_deref(), Some(&b"B"[..]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rolling_quick_moves_and_drops_thumbnails_with_the_state() {
        let dir = temp_path("rollthumb");
        std::fs::create_dir_all(&dir).unwrap();
        let rom = Path::new("/roms/mario.nes");
        let core = "mesen";
        for save in [b"A" as &[u8], b"B", b"C", b"D"] {
            roll_quick(&dir, rom, core);
            write(&quick_state_path(&dir, rom, core, 0), save).unwrap();
            write(&quick_state_thumb_path(&dir, rom, core, 0), save).unwrap();
        }
        // D, C, B remain, and each thumbnail followed its state.
        for (rank, expected) in [(0u8, &b"D"[..]), (1, &b"C"[..]), (2, &b"B"[..])] {
            assert_eq!(
                read(&quick_state_path(&dir, rom, core, rank)).as_deref(),
                Some(expected)
            );
            assert_eq!(
                read(&quick_state_thumb_path(&dir, rom, core, rank)).as_deref(),
                Some(expected)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_legacy_quick_drops_state0_but_keeps_manual_slots() {
        let dir = temp_path("legacy");
        std::fs::create_dir_all(&dir).unwrap();
        // The old single quick slot, its thumbnail, and a still-valid manual
        // slot (1) plus a quick saveq file (already the new layout).
        std::fs::write(dir.join("mario.nes.mesen.state0"), b"old").unwrap();
        std::fs::write(dir.join("mario.nes.mesen.state0.png"), b"old").unwrap();
        std::fs::write(dir.join("mario.nes.mesen.state1"), b"manual").unwrap();
        std::fs::write(dir.join("mario.nes.mesen.stateq0"), b"quick").unwrap();

        remove_legacy_quick(&dir);

        assert!(!dir.join("mario.nes.mesen.state0").exists());
        assert!(!dir.join("mario.nes.mesen.state0.png").exists());
        assert!(dir.join("mario.nes.mesen.state1").exists());
        assert!(dir.join("mario.nes.mesen.stateq0").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compacting_after_delete_keeps_the_stack_contiguous() {
        let dir = temp_path("compact");
        std::fs::create_dir_all(&dir).unwrap();
        let rom = Path::new("/roms/mario.nes");
        let core = "mesen";
        quick_push(&dir, rom, core, b"A");
        quick_push(&dir, rom, core, b"B");
        quick_push(&dir, rom, core, b"C"); // [C, B, A]

        // Delete the middle (B); the older A moves up into rank 1.
        compact_quick(&dir, rom, core, 1);
        assert_eq!(
            read(&quick_state_path(&dir, rom, core, 0)).as_deref(),
            Some(&b"C"[..])
        );
        assert_eq!(
            read(&quick_state_path(&dir, rom, core, 1)).as_deref(),
            Some(&b"A"[..])
        );
        assert!(!list_quick_slots(&dir, rom, core)
            .iter()
            .any(|slot| slot.slot == 2 && slot.exists));

        // Deleting the newest (C) promotes the next save to newest.
        compact_quick(&dir, rom, core, 0);
        assert_eq!(
            read(&quick_state_path(&dir, rom, core, 0)).as_deref(),
            Some(&b"A"[..])
        );
        assert!(read(&quick_state_path(&dir, rom, core, 1)).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
