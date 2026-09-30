//! Cover / icon / screenshot / save texture registration — methods on [`super::App`].

use super::*;

impl super::App {
    /// Decode and upload a texture for each game whose cover changed, reusing
    /// the previous upload when it did not. Covers whose game is gone are
    /// dropped from the cache (their texture stays on the GPU — the backend
    /// has no remove).
    pub(super) fn refresh_cover_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        let Some(library) = &self.library else {
            return;
        };
        let live: HashSet<i64> = self.game_source.iter().map(|game| game.id).collect();
        // Games that vanished (or whose cover was cleared) no longer have an
        // image; free their GPU texture instead of leaving it resident until the
        // process exits. The backend keeps uploaded textures until told.
        let removed: Vec<i64> = self
            .cover_textures
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in removed {
            backend.remove_texture(TextureId::new(COVER_TEXTURE_BASE + id as u32));
            self.cover_textures.remove(&id);
        }

        for game in &self.game_source {
            let Some(cover_id) = game.cover else {
                if self.cover_textures.remove(&game.id).is_some() {
                    backend.remove_texture(TextureId::new(COVER_TEXTURE_BASE + game.id as u32));
                }
                continue;
            };
            if self
                .cover_textures
                .get(&game.id)
                .is_some_and(|cover| cover.cover_id == cover_id)
            {
                continue;
            }
            let Ok(Some(path)) = library.screenshot_path(cover_id) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok((width, height, rgba)) = decode_png(&bytes) else {
                continue;
            };
            let texture = TextureId::new(COVER_TEXTURE_BASE + game.id as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                self.cover_textures.insert(
                    game.id,
                    CoverTexture {
                        cover_id,
                        handle: FrameHandle {
                            texture,
                            width,
                            height,
                        },
                    },
                );
            }
        }
    }

    /// Decode and upload a texture for every icon, so the UI draws each one as
    /// a single image instead of re-stroking its SVG every frame.
    pub(super) fn install_icon_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        for (index, name) in crate::ui::IconName::ALL.into_iter().enumerate() {
            let Some((width, height, rgba)) = crate::ui::rasterize_icon(name, ICON_TEXTURE_PX)
            else {
                continue;
            };
            let texture = TextureId::new(ICON_TEXTURE_BASE + index as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                crate::ui::set_texture(
                    name,
                    FrameHandle {
                        texture,
                        width,
                        height,
                    },
                );
            }
        }
    }

    /// Decode and upload a texture for every screenshot, reusing uploads
    /// whose file is unchanged. The backend has no `remove_texture`, so
    /// deleted screenshots leave their texture behind.
    pub(super) fn refresh_screenshot_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        let Some(library) = &self.library else {
            return;
        };
        let shots = library.screenshots().unwrap_or_default();
        let live: HashSet<i64> = shots.iter().map(|shot| shot.id).collect();
        let removed: Vec<i64> = self
            .screenshot_textures
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in removed {
            backend.remove_texture(TextureId::new(SCREENSHOT_TEXTURE_BASE + id as u32));
            self.screenshot_textures.remove(&id);
        }

        for shot in &shots {
            if self
                .screenshot_textures
                .get(&shot.id)
                .is_some_and(|texture| texture.file == shot.file)
            {
                continue;
            }
            let Ok(Some(path)) = library.screenshot_path(shot.id) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok((width, height, rgba)) = decode_png(&bytes) else {
                continue;
            };
            let texture = TextureId::new(SCREENSHOT_TEXTURE_BASE + shot.id as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                self.screenshot_textures.insert(
                    shot.id,
                    ScreenshotTexture {
                        file: shot.file.clone(),
                        handle: FrameHandle {
                            texture,
                            width,
                            height,
                        },
                    },
                );
            }
        }
    }
}
