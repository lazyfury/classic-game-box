//! Game library: scan, import, sort, pin, delete, rename — methods on [`super::App`].

use super::*;

impl super::App {
    /// Re-read the library folders and rebuild the game rows.
    ///
    /// The folder is the truth about what exists, so every refresh rescans it
    /// and reconciles the database: new files are inserted, vanished files are
    /// dropped, changed files have their facts refreshed. The rows then come
    /// from the database, which is the model — the name, pin, play statistics,
    /// screenshots and cover a scan cannot know live there.
    pub(super) fn refresh_library(&mut self) {
        // The one game library (once chosen) plus its built-in ROM folder.
        // Before a library is chosen the game data lives in app data, so only
        // the built-in folder is scanned there — walking app data itself would
        // trip over the BIOS folder.
        let mut dirs: Vec<PathBuf> = Vec::new();
        if self.paths.has_library() {
            dirs.push(self.paths.root.clone());
        }
        if !dirs.contains(&self.paths.roms) {
            dirs.push(self.paths.roms.clone());
        }

        // Individually added files (dragged in or chosen in the dialog) merge
        // in, and are pruned from the settings once their file is gone.
        let (disk, kept) = collect_games(&dirs, &self.settings.added_roms);
        if kept != self.settings.added_roms {
            self.settings.added_roms = kept;
            let _ = self.settings.save(&self.paths.settings_json);
        }

        self.game_source = match &self.library {
            Some(library) => {
                let _ = library.sync(&disk);
                library.games().unwrap_or_default()
            }
            // No database: fall back to the scan, with no metadata to show.
            None => disk.iter().map(Game::from_disk).collect(),
        };
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();
        self.rebuild_missing_cores();
    }

    /// Rebuild the view from the database without rescanning the ROM folders.
    ///
    /// Screenshot and cover changes only touch the database and the files under
    /// the screenshots directory, so a full folder scan is wasted work.
    pub(super) fn reload_from_db(&mut self) {
        self.game_source = self
            .library
            .as_ref()
            .and_then(|library| library.games().ok())
            .unwrap_or_default();
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();
    }

    /// Choose `dir` as the game library: remember it, point the library data
    /// (database, screenshots, saves, cheats) at it and rescan. There is only
    /// one library, so this replaces whatever was open.
    pub(super) fn set_library_root(&mut self, dir: PathBuf) {
        let dir = dir.to_string_lossy().into_owned();
        self.settings.library_root = Some(dir.clone());
        let _ = self.settings.save(&self.paths.settings_json);
        self.paths = Paths::new(self.paths.user_data.clone(), Some(PathBuf::from(&dir)));
        let _ = self.paths.ensure();
        self.library = Library::open(&self.paths.library_db).ok();
        self.reload_library();
        self.model
            .set_status(format!("已切换游戏库：{dir}"), StatusKind::Info);
    }

    /// Ask for a folder and switch to it as the game library.
    pub(super) fn switch_library(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("选择游戏库").pick_folder() else {
            return;
        };
        self.set_library_root(dir);
    }

    /// Ask for ROM files and add them to the library.
    pub(super) fn add_games_dialog(&mut self) {
        let extensions: Vec<&str> = cgb_libretro::SYSTEMS
            .iter()
            .flat_map(|system| system.extensions().iter().copied())
            .collect();
        let Some(paths) = rfd::FileDialog::new()
            .set_title("选择游戏")
            .add_filter("ROM", &extensions)
            .pick_files()
        else {
            return;
        };
        self.add_game_paths(paths);
    }

    /// Add ROM files (dropped in, or picked in the dialog) to the library.
    ///
    /// Every ROM — one picked, or all of them inside a dropped folder — is
    /// **copied** into the library folder, so the library stays one
    /// self-contained folder. This is the legacy front end's rule
    /// (`Library.add`): a game that was only pointed at would break the moment
    /// its file moved. With no library chosen yet, the first dropped folder
    /// simply becomes the library.
    pub(super) fn add_game_paths(&mut self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        let mut switched = false;
        for path in paths {
            if path.is_dir() {
                if !self.paths.has_library() {
                    // Nothing chosen yet: the folder becomes the library.
                    self.set_library_root(path);
                    switched = true;
                    continue;
                }
                if inside_library(&self.paths.root, &path) {
                    // Already part of the library; the rescan finds it.
                    continue;
                }
                files.extend(
                    crate::library::scan_dir(&path)
                        .into_iter()
                        .map(|game| PathBuf::from(game.path)),
                );
            } else {
                files.push(path);
            }
        }
        let report = import_roms(&self.paths.roms, &files);
        self.reload_library();
        self.queue_missing_core_prompt(&report.copied);
        if !report.is_empty() || !switched {
            self.model
                .set_status(import_status(&report), StatusKind::Info);
        }
    }

    /// If a just-added ROM needs a console with no available core, queue the
    /// "download a core?" prompt for the next frame. Opening it here would be
    /// undone by the action dispatch that follows (`handle_actions` closes
    /// overlays it did not open).
    fn queue_missing_core_prompt(&mut self, added: &[PathBuf]) {
        let added_systems: Vec<SystemId> = added
            .iter()
            .map(|path| system_for_path(&path.to_string_lossy()))
            .collect();
        let names: Vec<String> = self
            .model
            .missing_cores
            .iter()
            .filter(|row| added_systems.contains(&row.system))
            .map(|row| row.system.name().to_string())
            .collect();
        if !names.is_empty() {
            self.pending_core_prompt = Some(format!(
                "新增的游戏需要 {} 的核心，现在下载吗？",
                names.join("、")
            ));
        }
    }

    /// Add any files dropped since the last frame, in one batch.
    pub(super) fn flush_drops(&mut self) {
        if self.pending_drops.borrow().is_empty() {
            return;
        }
        let paths = std::mem::take(&mut *self.pending_drops.borrow_mut());
        self.add_game_paths(paths);
    }

    /// Pin or unpin a game, then re-sort so it moves to (or leaves) the top.
    pub(super) fn toggle_pin(&mut self, index: usize) {
        let Some(game) = self.model.games.get(index) else {
            return;
        };
        let path = game.path.clone();
        let pinned = !game.pinned;
        if let Some(library) = &self.library {
            let _ = library.set_pinned(&path, pinned);
        }
        if let Some(source) = self.game_source.iter_mut().find(|game| game.path == path) {
            source.pinned = pinned;
        }
        self.rebuild_game_rows();
        let verb = if pinned {
            "已置顶"
        } else {
            "已取消置顶"
        };
        let message = format!("{verb}：{}", self.name_of(&path));
        self.model.set_status(message, StatusKind::Info);
    }

    /// Delete a game outright: remove the ROM file, forget it in the settings
    /// and the database. If it was the running game, stop the machine first.
    pub(super) fn delete_game(&mut self, game_id: i64) {
        let Some(game) = self.game_source.iter().find(|game| game.id == game_id) else {
            return;
        };
        let path = game.path.clone();
        let name = game.name.clone();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                self.model
                    .set_status(format!("删除失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        }
        if self.selected_game_id() == Some(game_id) {
            self.flush_playtime();
            self.session = None;
            self.cheats.clear();
            self.cheat_path = None;
            self.core_options.clear();
            self.model.selected = None;
            self.model.has_session = false;
            self.model.paused = false;
            self.model.frame = None;
            // The immersive view has nothing left to show.
            self.set_fullscreen(false);
        }
        self.settings.added_roms.retain(|rom| rom != &path);
        let _ = self.settings.save(&self.paths.settings_json);
        if let Some(library) = &self.library {
            let _ = library.remove(&path);
        }
        self.reload_library();
        self.model
            .set_status(format!("已删除：{name}"), StatusKind::Success);
    }

    /// The display name of a library path, for status messages; falls back to
    /// the path itself when the game is no longer listed.
    pub(super) fn name_of(&self, path: &str) -> String {
        self.model
            .games
            .iter()
            .find(|game| game.path == path)
            .map(|game| game.name.clone())
            .unwrap_or_else(|| path.to_string())
    }

    /// Pick a sort key. Choosing the active key again flips the direction;
    /// choosing a new key starts it in that key's natural direction.
    pub(super) fn set_sort(&mut self, key: SortKey) {
        if self.model.sort == key {
            self.settings.library_sort_desc = !self.settings.library_sort_desc;
        } else {
            self.settings.library_sort = key.key().to_string();
            self.settings.library_sort_desc = key.default_desc();
        }
        let _ = self.settings.save(&self.paths.settings_json);
        self.rebuild_game_rows();
    }

    /// Re-scan and reconcile the library after the folders changed.
    pub(super) fn reload_library(&mut self) {
        self.refresh_library();
        self.rebuild_settings_view();
    }

    /// Remember which console a game runs as, overriding the one its extension
    /// suggests. The library is the source of truth; the in-memory rows are
    /// refreshed so the badge, tallies and system filter follow.
    pub(super) fn set_game_system(&mut self, id: i64, system: SystemId) {
        let Some(path) = self
            .game_source
            .iter()
            .find(|game| game.id == id)
            .map(|game| game.path.clone())
        else {
            return;
        };
        if let Some(library) = &self.library {
            if let Err(error) = library.set_system(&path, system) {
                self.model
                    .set_status(format!("设置机种失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        }
        if let Some(game) = self.game_source.iter_mut().find(|game| game.id == id) {
            game.system = system;
        }
        self.rebuild_game_rows();
        self.model
            .set_status(format!("已设为 {}", system.name()), StatusKind::Success);
        self.dirty = true;
    }

    /// Rebuild the library rows from [`App::game_source`], applying the saved
    /// sort order. Pinned games always come first; the sort key only orders
    /// within the pinned and unpinned groups. Cheap enough to run on every sort
    /// click, since it does not touch disk.
    pub(super) fn rebuild_game_rows(&mut self) {
        let key = SortKey::from_key(&self.settings.library_sort);
        let desc = self.settings.library_sort_desc;
        // The selection is an index into the rows, so reordering moves it.
        // Remember the path and re-point the index after the sort.
        let selected = self
            .model
            .selected
            .and_then(|index| self.model.games.get(index))
            .map(|game| game.path.clone());
        // The tallies are over the whole library, so they do not move as the
        // search or the system filter narrows the grid. A filter for a console
        // the library no longer holds (after switching libraries) is dropped.
        self.model.total_games = self.game_source.len();
        self.model.system_counts = system_counts(&self.game_source);
        if let Some(filter) = self.model.system_filter {
            if !self
                .model
                .system_counts
                .iter()
                .any(|tally| tally.system == filter)
            {
                self.model.system_filter = None;
            }
        }
        let mut games = self.game_source.clone();
        if let Some(system) = self.model.system_filter {
            games.retain(|game| game.system == system);
        }
        if !self.model.search.is_empty() {
            games.retain(|game| game_matches_search(game, &self.model.search));
        }
        order_games(&mut games, key, desc);
        let covers = &self.cover_textures;
        self.model.games = games
            .into_iter()
            .map(|game| {
                let cover = covers.get(&game.id).map(|cover| cover.handle);
                game_row(game, cover)
            })
            .collect();
        self.model.selected =
            selected.and_then(|path| self.model.games.iter().position(|game| game.path == path));
        self.model.sort = key;
        self.model.sort_desc = desc;
        self.dirty = true;
    }

    /// A single click on a card. Two on the same card within a short window are
    /// a double click, which starts the game — the card itself no longer plays
    /// on one click, so it cannot fight the context menu. The "立即游玩" button
    /// is the deliberate one-click path.
    pub(super) fn card_activate(&mut self, index: usize) {
        const DOUBLE_CLICK_MS: i64 = 400;
        let now = now_millis();
        let is_double = matches!(
            self.card_click,
            Some((last, at)) if last == index && now - at <= DOUBLE_CLICK_MS
        );
        if is_double {
            self.card_click = None;
            self.start_game(index);
        } else {
            self.card_click = Some((index, now));
        }
    }
}
