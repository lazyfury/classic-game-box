//! Screenshots page: grid, preview, cover, batch delete — methods on [`super::App`].

use super::*;

impl super::App {
    /// Build the screenshots section's rows: the library's screenshots with
    /// their game name, cover flag and thumbnail handle.
    pub(super) fn rebuild_screenshot_rows(&mut self) {
        let Some(library) = &self.library else {
            self.model.screenshots.clear();
            return;
        };
        let names: HashMap<i64, String> = self
            .game_source
            .iter()
            .map(|game| (game.id, game.name.clone()))
            .collect();
        let covers: HashMap<i64, Option<i64>> = self
            .game_source
            .iter()
            .map(|game| (game.id, game.cover))
            .collect();
        let shots = library.screenshots().unwrap_or_default();
        self.model.screenshots = shots
            .into_iter()
            .map(|shot| ScreenshotRow {
                id: shot.id,
                game_id: shot.game_id,
                game: names.get(&shot.game_id).cloned().unwrap_or_default(),
                created_at: shot.created_at,
                is_cover: covers.get(&shot.game_id).copied().flatten() == Some(shot.id),
                thumb: self
                    .screenshot_textures
                    .get(&shot.id)
                    .map(|texture| texture.handle),
            })
            .collect();
        // A preview whose screenshot is gone (deleted, or its game removed)
        // closes, resuming a game it paused.
        if let Some(id) = self.model.preview {
            if !self.model.screenshots.iter().any(|shot| shot.id == id) {
                self.model.preview = None;
                if self.preview_paused {
                    if let Some(session) = self.session.as_mut() {
                        session.toggle_pause();
                    }
                    self.preview_paused = false;
                }
            }
        }
        self.model.screenshot_select = self.screenshot_select;
        self.model.selected_screenshots = self.selected_shots.clone();
    }

    /// The id of the currently selected (usually playing) game.
    pub(super) fn selected_game_id(&self) -> Option<i64> {
        self.model
            .selected
            .and_then(|index| self.model.games.get(index))
            .map(|game| game.id)
    }

    /// The screenshots the section currently shows: the game's, newest first.
    pub(super) fn visible_screenshots(&self) -> Vec<i64> {
        let game_id = self
            .model
            .screenshot_game
            .or_else(|| self.selected_game_id());
        self.model
            .screenshots
            .iter()
            .filter(|shot| Some(shot.game_id) == game_id)
            .map(|shot| shot.id)
            .collect()
    }

    /// Show a screenshot large in the play column, pausing a running game.
    pub(super) fn preview_screenshot(&mut self, id: i64) {
        self.model.preview = Some(id);
        if let Some(session) = self.session.as_mut() {
            if !session.paused() {
                session.toggle_pause();
                self.preview_paused = true;
                self.flush_playtime();
            }
        }
        self.dirty = true;
    }

    /// Close the preview, resuming a game this preview paused.
    pub(super) fn close_preview(&mut self) {
        self.model.preview = None;
        if self.preview_paused {
            if let Some(session) = self.session.as_mut() {
                session.toggle_pause();
            }
            self.preview_paused = false;
        }
        self.dirty = true;
    }

    /// Step the preview to the next (`+1`) or previous (`-1`) screenshot.
    pub(super) fn step_preview(&mut self, delta: i32) {
        let ids = self.visible_screenshots();
        if ids.is_empty() {
            return;
        }
        let current = self
            .model
            .preview
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0);
        let last = ids.len() as i32 - 1;
        let next = (current as i32 + delta).clamp(0, last) as usize;
        self.model.preview = Some(ids[next]);
        self.dirty = true;
    }

    /// Make a screenshot its game's cover.
    pub(super) fn set_cover(&mut self, id: i64) {
        if let Some(library) = &self.library {
            let _ = library.set_cover(id);
        }
        self.reload_from_db();
        self.model
            .set_status("已设为封面".to_string(), StatusKind::Success);
    }

    /// Delete a screenshot (row and file).
    pub(super) fn remove_screenshot(&mut self, id: i64) {
        if let Some(library) = &self.library {
            let _ = library.remove_screenshot(id);
        }
        if self.model.preview == Some(id) {
            self.close_preview();
        }
        self.reload_from_db();
        self.model
            .set_status("已删除截图".to_string(), StatusKind::Success);
    }

    /// Delete every ticked screenshot, then leave select mode.
    pub(super) fn delete_selected_screenshots(&mut self) {
        let count = self.selected_shots.len();
        if count == 0 {
            return;
        }
        if let Some(library) = &self.library {
            for id in &self.selected_shots {
                let _ = library.remove_screenshot(*id);
            }
        }
        self.selected_shots.clear();
        self.screenshot_select = false;
        self.reload_from_db();
        self.model
            .set_status(format!("已删除 {count} 张截图"), StatusKind::Success);
    }

    /// Reveal a screenshot's file in the platform file browser.
    pub(super) fn reveal_screenshot(&mut self, id: i64) {
        let Some(library) = &self.library else {
            return;
        };
        if let Ok(Some(path)) = library.screenshot_path(id) {
            reveal_path(&path);
        }
    }

    /// Open the screenshots directory in the platform file browser.
    pub(super) fn open_screenshots_folder(&mut self) {
        if let Some(library) = &self.library {
            open_path(library.screenshots_dir());
        }
    }

    /// Take a screenshot of the running game: encode its last frame, store it
    /// under the game in the library, and refresh. `as_cover` also makes the
    /// new picture the game's cover.
    pub(super) fn capture_screenshot(&mut self, as_cover: bool) {
        let Some(session) = self.session.as_ref() else {
            self.model
                .set_status("没有正在运行的游戏".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let rom_path = session.rom_path().to_string_lossy().into_owned();
        let Some((width, height, pixels)) = session.last_pixels() else {
            self.model
                .set_status("还没有画面可以截图".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let png = match encode_png(width, height, pixels) {
            Ok(png) => png,
            Err(error) => {
                self.model
                    .set_status(format!("截图失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };
        let saved = match &self.library {
            Some(library) => library.save_screenshot(
                &rom_path,
                &png,
                i64::from(width),
                i64::from(height),
                as_cover,
            ),
            None => {
                self.model
                    .set_status("游戏库不可用".to_string(), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };
        match saved {
            Ok(Some(_)) => {
                let message = if as_cover {
                    "已截图并设为封面"
                } else {
                    "已截图"
                };
                self.model.set_status(message, StatusKind::Success);
                self.reload_from_db();
            }
            Ok(None) => {
                self.model
                    .set_status("该游戏不在游戏库中".to_string(), StatusKind::Info);
                self.dirty = true;
            }
            Err(error) => {
                self.model
                    .set_status(format!("截图失败：{error}"), StatusKind::Error);
                self.dirty = true;
            }
        }
    }
}
