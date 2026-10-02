//! Save-state slots — methods on [`super::App`].

use super::*;

impl super::App {
    /// Write a save state to a fixed manual slot.
    pub(super) fn save_to_slot(&mut self, slot: u8) {
        self.flush_playtime();
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.save_state(slot) {
                Ok(()) => (
                    format!("已存档（{}）", manual_slot_label(slot)),
                    StatusKind::Success,
                ),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.refresh_saves();
        self.dirty = true;
    }

    /// Load a save state from a fixed manual slot.
    pub(super) fn load_from_slot(&mut self, slot: u8) {
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.load_state(slot) {
                Ok(()) => (
                    format!("已读档（{}）", manual_slot_label(slot)),
                    StatusKind::Success,
                ),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.dirty = true;
    }

    /// Delete a fixed manual slot's state and thumbnail.
    pub(super) fn delete_slot(&mut self, slot: u8) {
        if let Some(session) = self.session.as_ref() {
            session.delete_save(slot);
        }
        self.model.set_status(
            format!("已删除存档（{}）", manual_slot_label(slot)),
            StatusKind::Success,
        );
        self.refresh_saves();
        self.dirty = true;
    }

    /// Quick-save into the newest rolling slot (`fast01`), pushing the older
    /// saves down a rank and dropping the oldest.
    pub(super) fn quick_save(&mut self) {
        self.flush_playtime();
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.quick_save() {
                Ok(()) => (
                    format!("已快速存档（{}）", quick_slot_label(0)),
                    StatusKind::Success,
                ),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.refresh_saves();
        self.dirty = true;
    }

    /// Load the rolling quick save at `rank` (`0` = newest). Reading does not
    /// remove it, so it can be loaded again.
    pub(super) fn load_quick(&mut self, rank: u8) {
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.quick_load(rank) {
                Ok(()) => (
                    format!("已读档（{}）", quick_slot_label(rank)),
                    StatusKind::Success,
                ),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.dirty = true;
    }

    /// Delete the rolling quick save at `rank`, compacting the rest.
    pub(super) fn delete_quick(&mut self, rank: u8) {
        if let Some(session) = self.session.as_ref() {
            session.delete_quick(rank);
        }
        self.model.set_status(
            format!("已删除存档（{}）", quick_slot_label(rank)),
            StatusKind::Success,
        );
        self.refresh_saves();
        self.dirty = true;
    }

    /// Rebuild both save lists for the running game and its core, uploading a
    /// thumbnail for any entry that changed.
    pub(super) fn refresh_saves(&mut self) {
        self.model.quick_saves.clear();
        self.model.save_states.clear();
        self.model.save_states_supported = false;
        let Some(session) = self.session.as_ref() else {
            return;
        };
        self.model.save_states_supported = session.save_supported();

        for slot in session.quick_slots() {
            let key = SaveThumb::Quick(slot.slot);
            let path = session.quick_thumbnail_path(slot.slot);
            let thumb =
                upload_save_thumb(&mut self.save_textures, &self.backend, key, &slot, &path);
            self.model.quick_saves.push(SaveSlotRow {
                slot: slot.slot,
                exists: slot.exists,
                modified_ms: slot.modified_ms,
                thumb,
            });
        }
        for slot in session.save_slots() {
            let key = SaveThumb::Manual(slot.slot);
            let path = session.save_thumbnail_path(slot.slot);
            let thumb =
                upload_save_thumb(&mut self.save_textures, &self.backend, key, &slot, &path);
            self.model.save_states.push(SaveSlotRow {
                slot: slot.slot,
                exists: slot.exists,
                modified_ms: slot.modified_ms,
                thumb,
            });
        }
    }
}

/// Upload a slot's thumbnail when it changed, and return the registered handle.
fn upload_save_thumb(
    textures: &mut HashMap<SaveThumb, (i64, TextureHandle)>,
    backend: &Option<SharedBackend>,
    key: SaveThumb,
    slot: &StateSlot,
    path: &Path,
) -> Option<TextureHandle> {
    if !slot.thumbnail {
        return None;
    }
    let stale = textures.get(&key).map(|(modified, _)| *modified) != Some(slot.modified_ms);
    if stale {
        if let Some(backend) = backend.clone() {
            let mut backend = backend.borrow_mut();
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok((width, height, rgba)) = decode_png(&bytes) {
                    let texture = key.texture_id();
                    if backend
                        .register_texture(texture, width, height, &rgba)
                        .is_ok()
                    {
                        textures.insert(
                            key,
                            (
                                slot.modified_ms,
                                TextureHandle {
                                    texture,
                                    width,
                                    height,
                                },
                            ),
                        );
                    }
                }
            }
        }
    }
    textures.get(&key).map(|(_, handle)| *handle)
}
