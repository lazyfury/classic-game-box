//! Save-state slots — methods on [`super::App`].

use super::*;

impl super::App {
    /// Write a save state to a slot, with a thumbnail.
    pub(super) fn save_to_slot(&mut self, slot: u8) {
        self.flush_playtime();
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.save_state(slot) {
                Ok(()) => (format!("已存档（槽位 {}）", slot + 1), StatusKind::Success),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.refresh_saves();
        self.dirty = true;
    }

    /// Load a save state from a slot.
    pub(super) fn load_from_slot(&mut self, slot: u8) {
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.load_state(slot) {
                Ok(()) => (format!("已读档（槽位 {}）", slot + 1), StatusKind::Success),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.dirty = true;
    }

    /// Delete a slot's state and thumbnail.
    pub(super) fn delete_slot(&mut self, slot: u8) {
        if let Some(session) = self.session.as_ref() {
            session.delete_save(slot);
        }
        self.model.set_status(
            format!("已删除存档（槽位 {}）", slot + 1),
            StatusKind::Success,
        );
        self.refresh_saves();
        self.dirty = true;
    }

    /// Rebuild the saves list for the running game and its core, uploading a
    /// thumbnail for any slot that changed.
    pub(super) fn refresh_saves(&mut self) {
        self.model.saves.clear();
        self.model.saves_supported = false;
        let Some(session) = self.session.as_ref() else {
            return;
        };
        self.model.saves_supported = session.save_supported();
        let slots = session.save_slots();
        for slot in &slots {
            let mut thumb = None;
            if slot.thumbnail {
                let stale = self
                    .save_textures
                    .get(&slot.slot)
                    .map(|(modified, _)| *modified)
                    != Some(slot.modified_ms);
                if stale {
                    if let Some(backend) = self.backend.clone() {
                        let mut backend = backend.borrow_mut();
                        if let Ok(bytes) = std::fs::read(session.save_thumbnail_path(slot.slot)) {
                            if let Ok((width, height, rgba)) = decode_png(&bytes) {
                                let texture = TextureId::new(SAVE_TEXTURE_BASE + slot.slot as u32);
                                if backend
                                    .register_texture(texture, width, height, &rgba)
                                    .is_ok()
                                {
                                    self.save_textures.insert(
                                        slot.slot,
                                        (
                                            slot.modified_ms,
                                            FrameHandle {
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
                thumb = self
                    .save_textures
                    .get(&slot.slot)
                    .map(|(_, handle)| *handle);
            }
            self.model.saves.push(SaveSlotRow {
                slot: slot.slot,
                exists: slot.exists,
                modified_ms: slot.modified_ms,
                thumb,
            });
        }
    }
}
