//! Keyboard / gamepad routing and action handling — methods on [`super::App`].

use super::*;

impl super::App {
    /// App-level keyboard commands, handled before the UI or the emulator sees
    /// the key. While a text field is open it owns the keyboard, except that
    /// Enter commits and Escape cancels.
    pub(super) fn handle_hotkey(&mut self, event: &InputEvent) {
        let (key, pressed) = match event {
            InputEvent::KeyDown { key } => (*key, true),
            InputEvent::KeyUp { key } => (*key, false),
            _ => return,
        };
        if self.model.editing.is_some() {
            if pressed {
                match key {
                    Key::Enter => self.commit_edit(),
                    Key::Escape => self.cancel_edit(),
                    _ => {}
                }
            }
            return;
        }
        match key {
            // Backspace is the rewind key: hold it to step the game back.
            Key::Backspace => self.rewinding = pressed,
            // F12 is the screenshot key; Shift+F12 also sets the cover.
            Key::F12 if pressed => self.capture_screenshot(self.modifiers.shift),
            // Save-state / fullscreen hotkeys fire once, on press.
            _ if pressed => {
                if let Some(action) = state_shortcut(key, self.modifiers.shift) {
                    self.actions.push(action);
                    self.handle_actions();
                }
            }
            _ => {}
        }
    }

    /// Route one input event: the UI first, then the emulator bindings.
    pub(super) fn feed(&mut self, event: &InputEvent) {
        let scroll_before = self.ui.scroll_offset();
        let captured = self
            .session
            .as_ref()
            .is_some_and(|session| !session.paused());
        let keyboard = matches!(event, InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. });
        let editing = self.model.editing.is_some();
        // An open overlay owns the event first, even while a game captures the
        // keyboard: a menu opened from the UI must close on Escape.
        if !self.ui.route_overlay_input(event) {
            if editing {
                // An open text field owns the keyboard (text, arrows, IME);
                // Enter / Escape were already handled as app commands.
                self.ui.route_ui_input(event);
            } else if captured && keyboard {
                // The running game owns the keyboard; Escape is the one way
                // back to the UI.
                if matches!(event, InputEvent::KeyDown { key: Key::Escape }) {
                    self.escape_game();
                } else if let Some(bindings) = self.bindings.get(&self.active_system) {
                    match event {
                        InputEvent::KeyDown { key } => {
                            if let Some(key) = to_input_key(*key) {
                                bindings.apply(key, true, &mut self.input, 0);
                            }
                        }
                        InputEvent::KeyUp { key } => {
                            if let Some(key) = to_input_key(*key) {
                                bindings.apply(key, false, &mut self.input, 0);
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                // No game, or paused: the UI owns the keyboard. Tab moves the
                // focus; Enter / Space activate the focused control.
                if !self.ui.route_ui_input(event) {
                    if let InputEvent::KeyDown { key } = event {
                        match key {
                            Key::Tab => {
                                self.ui.move_focus(self.modifiers.shift);
                            }
                            Key::Enter | Key::Space => {
                                self.ui.activate_focus();
                            }
                            _ => {}
                        }
                    }
                }
                if matches!(event, InputEvent::KeyDown { key: Key::Escape }) {
                    self.escape_game();
                }
            }
        }
        self.handle_actions();
        // The resize handle owns the middle width. Feed the live value back so
        // the grid's column count follows it, and persist the width when the
        // drag ends.
        let content_width = self.ui.content_width();
        if content_width != self.model.content_width {
            self.model.content_width = content_width;
        }
        let released = matches!(event, InputEvent::PointerUp { .. });
        self.sync_grid_columns(released);
        if released && content_width != self.settings.content_width {
            self.settings.content_width = content_width;
            let _ = self.settings.save(&self.paths.settings_json);
        }
        // A wheel or scrollbar move changes the offset. Feed it back so the
        // grid can re-window; scrolling inside the mounted rows is a plain
        // repaint, so only mark the tree dirty when the window no longer
        // covers the viewport.
        let scroll_after = self.ui.scroll_offset();
        if matches!(self.model.section, Section::Library | Section::Screenshots)
            && !self.model.fullscreen
            && scroll_after != scroll_before
        {
            self.model.grid_offset = scroll_after;
            if !self.ui.grid_window_covers(&self.model) {
                self.dirty = true;
            }
        }
    }

    /// Step the grid's column count from the middle width, at most once per
    /// [`COLUMNS_CHECK_INTERVAL`] (or immediately when `force`). A change marks
    /// the UI dirty, so it rebuilds with the new column count.
    pub(super) fn sync_grid_columns(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_columns_check) < COLUMNS_CHECK_INTERVAL {
            return;
        }
        self.last_columns_check = now;
        let columns = library_columns(self.ui.content_width());
        if columns != self.model.grid_columns {
            self.model.grid_columns = columns;
            self.dirty = true;
        }
    }

    /// Act on whatever the UI recorded this event.
    pub(super) fn handle_actions(&mut self) {
        let actions = self.actions.drain();
        if actions.is_empty() {
            return;
        }
        let mut opened_overlay = false;
        for action in actions {
            match action {
                Action::GameContextMenu { index, position } => {
                    if let Some(game) = self.model.games.get(index) {
                        self.ui
                            .open_game_menu(self.theme, game, index, position, &self.actions);
                    }
                    opened_overlay = true;
                }
                Action::OpenCoreMenu { system, anchor } => {
                    self.ui.open_core_menu(
                        self.theme,
                        system,
                        anchor,
                        &self.model.cores,
                        &self.actions,
                    );
                    opened_overlay = true;
                }
                Action::OpenSystemMenu { id, position } => {
                    let current = self
                        .model
                        .games
                        .iter()
                        .find(|game| game.id == id)
                        .map(|game| game.system)
                        .unwrap_or(SystemId::Nes);
                    self.ui
                        .open_system_menu(self.theme, id, current, position, &self.actions);
                    opened_overlay = true;
                }
                Action::SetGameSystem { id, system } => self.set_game_system(id, system),
                Action::OpenGameCoreMenu { id, position } => {
                    if let Some(game) = self.model.games.iter().find(|game| game.id == id).cloned()
                    {
                        self.ui.open_game_core_menu(
                            self.theme,
                            &game,
                            position,
                            &self.model.cores,
                            &self.actions,
                        );
                    }
                    opened_overlay = true;
                }
                Action::SetGameCore { id, core } => self.set_game_core(id, core),
                Action::CardActivate(index) => self.card_activate(index),
                Action::Show(section) => {
                    let changed = self.model.section != section;
                    self.model.section = section;
                    // Leaving the page drops any in-progress edit.
                    self.model.editing = None;
                    // The screenshots section follows the playing game unless a
                    // card sent it to a specific one.
                    if section == Section::Screenshots && self.model.screenshot_game.is_none() {
                        self.model.screenshot_game = self.selected_game_id();
                    }
                    if section == Section::Saves {
                        self.refresh_saves();
                    }
                    if section == Section::Cheats {
                        if self.session.is_none() {
                            self.cheats.clear();
                            self.cheat_path = None;
                        }
                        self.populate_cheats();
                    }
                    // A page switch starts its scroll at the top; keep the
                    // grid's window in step with that.
                    if changed && matches!(section, Section::Library | Section::Screenshots) {
                        self.model.grid_offset = 0.0;
                    }
                    self.dirty = true;
                }
                Action::ShowSettingsGroup(group) => {
                    self.model.settings_group = group;
                    self.dirty = true;
                }
                Action::Play(index) => self.start_game(index),
                Action::TogglePause => {
                    if let Some(session) = self.session.as_mut() {
                        session.toggle_pause();
                    }
                    // A pause stops the clock, so bank what it has run.
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.paused())
                    {
                        self.flush_playtime();
                    }
                    self.dirty = true;
                }
                Action::ToggleFullscreen => self.toggle_fullscreen(),
                Action::Reset => {
                    if let Some(session) = self.session.as_ref() {
                        session.reset();
                    }
                }
                Action::Rewind => {
                    if let Some(session) = self.session.as_mut() {
                        if session.can_rewind() {
                            session.rewind_step();
                        }
                    }
                    self.ui.request_repaint();
                    self.dirty = true;
                }
                Action::SetShader(kind) => self.set_shader(kind),
                Action::SetMsaa(mode) => self.set_msaa(mode),
                Action::SetThemeChoice(choice) => self.set_theme(choice, self.light),
                Action::SetLight(light) => self.set_theme(self.theme_choice, light),
                Action::CycleCoreOption(index, delta) => self.cycle_core_option(index, delta),
                Action::AddGames => self.add_games_dialog(),
                Action::SwitchLibrary => self.switch_library(),
                Action::FilterSystem(system) => {
                    self.model.system_filter = system;
                    self.rebuild_game_rows();
                }
                Action::ToggleMissingCores => {
                    self.model.missing_cores_collapsed = !self.model.missing_cores_collapsed;
                    self.dirty = true;
                }
                Action::SelectCore(index) => self.select_core(index),
                Action::StartCatalogSearch => self.start_edit(EditTarget::CatalogSearch),
                Action::ClearCatalogSearch => self.clear_catalog_search(),
                Action::RefreshCatalog => self.refresh_catalog(),
                Action::DownloadCore(index) => {
                    if let Some(row) = self.model.catalog.get(index) {
                        let name = row.name.clone();
                        self.download_core(&name);
                    }
                }
                Action::DownloadRecommendedCore(system) => self.download_recommended_core(system),
                Action::DownloadMissingCores => self.download_missing_cores(),
                Action::TogglePin(index) => self.toggle_pin(index),
                Action::RequestDelete(confirm) => {
                    self.pending_confirm = Some(confirm);
                    self.ui.confirm_destructive(
                        confirm.title(),
                        confirm.message(),
                        Action::ConfirmDelete,
                        &self.actions,
                    );
                    opened_overlay = true;
                }
                Action::ConfirmDelete => self.confirm_delete(),
                Action::StartRename(id) => self.start_edit(EditTarget::GameName(id)),
                Action::StartTagEdit(id) => self.start_edit(EditTarget::GameTags(id)),
                Action::StartSearch => self.start_search(),
                Action::ClearSearch => self.clear_search(),
                Action::CommitEdit => self.commit_edit(),
                Action::CancelEdit => self.cancel_edit(),
                Action::Sort(key) => self.set_sort(key),
                Action::ToggleSortOrder => self.set_sort(self.model.sort),
                Action::Screenshot => self.capture_screenshot(false),
                Action::ScreenshotCover => self.capture_screenshot(true),
                Action::ShowScreenshots(game_id) => {
                    self.model.section = Section::Screenshots;
                    self.model.screenshot_game = Some(game_id);
                    self.dirty = true;
                }
                Action::PreviewScreenshot(id) => self.preview_screenshot(id),
                Action::ClosePreview => self.close_preview(),
                Action::StepPreview(delta) => self.step_preview(delta),
                Action::SetCover(id) => self.set_cover(id),
                Action::RevealScreenshot(id) => self.reveal_screenshot(id),
                Action::ToggleScreenshotSelect => {
                    self.screenshot_select = !self.screenshot_select;
                    if !self.screenshot_select {
                        self.selected_shots.clear();
                    }
                    self.rebuild_screenshot_rows();
                    self.dirty = true;
                }
                Action::ToggleScreenshotSelected(id) => {
                    if let Some(index) = self.selected_shots.iter().position(|shot| *shot == id) {
                        self.selected_shots.remove(index);
                    } else {
                        self.selected_shots.push(id);
                    }
                    self.rebuild_screenshot_rows();
                    self.dirty = true;
                }
                Action::DeleteSelectedScreenshots => self.delete_selected_screenshots(),
                Action::OpenScreenshotsFolder => self.open_screenshots_folder(),
                Action::SaveToSlot(slot) => self.save_to_slot(slot),
                Action::LoadFromSlot(slot) => self.load_from_slot(slot),
                Action::DeleteSlot(slot) => self.delete_slot(slot),
                Action::ImportCheats => self.import_cheats(),
                Action::ToggleCheat(index) => self.toggle_cheat(index),
            }
        }
        // A menu item action closes the menu; opening one keeps it.
        if !opened_overlay {
            self.ui.close_overlays();
        }
    }

    /// Run the destructive action the confirmation dialog was asking about.
    pub(super) fn confirm_delete(&mut self) {
        let Some(confirm) = self.pending_confirm.take() else {
            return;
        };
        self.dirty = true;
        match confirm {
            Confirm::DeleteGame(id) => self.delete_game(id),
            Confirm::DeleteScreenshot(id) => self.remove_screenshot(id),
        }
    }

    /// Escape while a game is running: pause it so the keyboard returns to the
    /// UI, or resume a paused one. In fullscreen it also leaves fullscreen, so
    /// Escape is always "give me back the window".
    pub(super) fn escape_game(&mut self) {
        let Some(paused) = self.session.as_mut().map(|session| {
            session.toggle_pause();
            session.paused()
        }) else {
            return;
        };
        if paused {
            self.flush_playtime();
        }
        self.model.paused = paused;
        self.model.set_status(
            if paused {
                "已暂停（再按 Esc 继续）"
            } else {
                "已继续"
            },
            StatusKind::Info,
        );
        if self.model.fullscreen {
            self.toggle_fullscreen();
        }
        self.dirty = true;
        self.ui.request_repaint();
    }

    /// Begin a text edit (a rename, a tag edit or a search); the app seeds the
    /// shared edit state the view mounts a `TextInput` from.
    pub(super) fn start_edit(&mut self, target: EditTarget) {
        let text = match target {
            EditTarget::LibrarySearch => self.model.search.clone(),
            EditTarget::CatalogSearch => self.model.catalog_query.clone(),
            EditTarget::GameName(id) | EditTarget::GameTags(id) => {
                let Some(game) = self.game_source.iter().find(|game| game.id == id) else {
                    return;
                };
                match target {
                    EditTarget::GameName(_) => game.name.clone(),
                    EditTarget::GameTags(_) => game.tags.join(", "),
                    EditTarget::LibrarySearch | EditTarget::CatalogSearch => {
                        unreachable!("handled above")
                    }
                }
            }
        };
        self.actions
            .set_edit(Rc::new(RefCell::new(TextEdit::new(text))));
        self.model.editing = Some(target);
        self.dirty = true;
    }

    /// Begin typing a library search; the app takes the keyboard.
    pub(super) fn start_search(&mut self) {
        self.start_edit(EditTarget::LibrarySearch);
    }

    /// Clear the search and leave any edit.
    pub(super) fn clear_search(&mut self) {
        self.model.search.clear();
        self.model.editing = None;
        self.actions.clear_edit();
        self.rebuild_game_rows();
        self.dirty = true;
    }

    /// Discard the pending edit. A search edit also clears the query.
    pub(super) fn cancel_edit(&mut self) {
        let target = self.model.editing.take();
        self.actions.clear_edit();
        match target {
            Some(EditTarget::LibrarySearch) => {
                self.model.search.clear();
                self.rebuild_game_rows();
            }
            Some(EditTarget::CatalogSearch) => {
                self.model.catalog_query.clear();
                self.rebuild_catalog();
            }
            _ => {}
        }
        self.dirty = true;
    }

    /// Commit the pending edit to the database and the in-memory rows.
    pub(super) fn commit_edit(&mut self) {
        let Some(target) = self.model.editing.take() else {
            return;
        };
        let text = self.actions.edit_text();
        self.actions.clear_edit();
        match target {
            // A search is applied as it is typed; committing just closes it.
            EditTarget::LibrarySearch => {
                let message = if self.model.search.is_empty() {
                    String::new()
                } else {
                    format!("搜索：{}", self.model.search)
                };
                self.model.set_status(message, StatusKind::Info);
            }
            EditTarget::CatalogSearch => {
                let message = if self.model.catalog_query.is_empty() {
                    String::new()
                } else {
                    format!("搜索核心：{}", self.model.catalog_query)
                };
                self.model.set_status(message, StatusKind::Info);
            }
            EditTarget::GameName(game_id) | EditTarget::GameTags(game_id) => {
                let Some(path) = self
                    .game_source
                    .iter()
                    .find(|game| game.id == game_id)
                    .map(|game| game.path.clone())
                else {
                    return;
                };
                match target {
                    EditTarget::GameName(_) => {
                        let name = text.trim().to_string();
                        if name.is_empty() {
                            self.model
                                .set_status("名字不能为空".to_string(), StatusKind::Error);
                            self.dirty = true;
                            return;
                        }
                        if let Some(library) = &self.library {
                            let _ = library.rename(&path, &name);
                        }
                        if let Some(game) =
                            self.game_source.iter_mut().find(|game| game.id == game_id)
                        {
                            game.name = name.clone();
                        }
                        self.model
                            .set_status(format!("已改名为：{name}"), StatusKind::Success);
                    }
                    EditTarget::GameTags(_) => {
                        let tags: Vec<String> = text
                            .split([',', '，', ' '])
                            .map(str::trim)
                            .filter(|tag| !tag.is_empty())
                            .map(str::to_string)
                            .collect();
                        if let Some(library) = &self.library {
                            let _ = library.set_tags(&path, &tags);
                        }
                        if let Some(game) =
                            self.game_source.iter_mut().find(|game| game.id == game_id)
                        {
                            game.tags = tags.clone();
                        }
                        self.model.set_status(
                            format!("已更新标签（{} 个）", tags.len()),
                            StatusKind::Success,
                        );
                    }
                    EditTarget::LibrarySearch | EditTarget::CatalogSearch => {
                        unreachable!("handled above")
                    }
                }
            }
        }
        self.rebuild_game_rows();
        self.dirty = true;
    }

    pub(super) fn start_game(&mut self, index: usize) {
        let Some(game) = self.model.games.get(index).cloned() else {
            return;
        };
        self.model.editing = None;
        self.model.selected = Some(index);
        self.start_path(Path::new(&game.path), game.system, game.core.as_deref());
    }

    /// Start a ROM by path: read it, pick a core, build a [`Session`]. `core`
    /// is the game's own core pick, if it has one.
    pub(super) fn start_path(&mut self, rom_path: &Path, system: SystemId, core: Option<&str>) {
        // The keyboard now feeds this console's binding set. The system comes
        // from the library row: a per-game pick can override the extension.
        self.active_system = system;
        let data = match std::fs::read(rom_path) {
            Ok(data) => data,
            Err(error) => {
                self.model
                    .set_status(format!("读取 ROM 失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };

        let spec = match self.resolve_core(system, core) {
            Ok(spec) => spec,
            Err(status) => {
                self.model.set_status(status, StatusKind::Info);
                self.dirty = true;
                return;
            }
        };

        if !spec.module.is_file() {
            self.model.set_status(
                format!(
                    "找不到核心 {}：先跑 ./scripts/build-cores.sh，或用 --core 指定模块",
                    spec.module.display()
                ),
                StatusKind::Error,
            );
            self.model.core_name = spec.name.clone();
            self.dirty = true;
            return;
        }

        // One core may be live at a time. The host publishes itself in a
        // process-wide slot and a core dylib is a single instance, so starting
        // the new session before the old one drops would `retro_init` the same
        // core again and then `retro_deinit` the new machine when the old
        // session falls (a segfault). Drop the old machine first; this also
        // flushes its battery save and its play time.
        self.flush_playtime();
        self.session = None;
        self.model.preview = None;
        self.preview_paused = false;

        let started = {
            let Some(shared) = self.backend.clone() else {
                return;
            };
            let mut backend = shared.borrow_mut();
            Session::start(
                &spec,
                &self.paths.system,
                &self.paths.saves,
                rom_path,
                &data,
                &mut backend,
            )
        };

        match started {
            Ok(session) => {
                self.model.core_name = session.core_name().to_string();
                self.model.has_session = true;
                self.model.paused = false;
                self.model.status.clear();
                self.session = Some(session);
                // Reset the on-screen FPS window for the new machine; show the
                // core's nominal rate until the first measured window.
                self.fps = self
                    .session
                    .as_ref()
                    .map(|session| session.target_fps())
                    .unwrap_or(0.0);
                self.fps_frames = self
                    .session
                    .as_ref()
                    .map(|session| session.frame_index())
                    .unwrap_or(0);
                self.fps_time = Instant::now();
                // Cheats are per game and applied right after load.
                let cheat_path = crate::paths::cheat_file(&self.paths.cheats, rom_path);
                self.cheats = crate::library::load_cheats(&cheat_path);
                self.cheat_path = Some(cheat_path);
                if let Some(session) = self.session.as_ref() {
                    session.apply_cheats(&self.cheats);
                }
                if !self.cheats.is_empty() {
                    self.model.set_status(
                        format!("已应用 {} 条金手指", self.cheats.len()),
                        StatusKind::Success,
                    );
                }
                self.populate_cheats();
                // The picture effect follows the session.
                self.apply_shader();
                // Core options are known only after load.
                self.reload_core_options();
                // Count the run and stamp it; this also re-points the selection
                // at the row, which a sort by "recent" may have moved.
                self.note_started(rom_path);
                // The saves list follows the running core.
                self.refresh_saves();
            }
            Err(error) => {
                // An arcade ROM usually fails because its BIOS `.zip` is not in
                // the system directory; say where to put it.
                let message = if self.active_system == SystemId::Arcade {
                    format!(
                        "{error}（街机 ROM 需要对应的 BIOS .zip，放到 {}）",
                        self.paths.system.display()
                    )
                } else {
                    error
                };
                self.model.set_status(message, StatusKind::Error);
                self.model.has_session = false;
                // A failed load leaves nothing to fill the immersive view.
                self.set_fullscreen(false);
            }
        }
        self.dirty = true;
    }

    pub(super) fn step_gamepad(&mut self) {
        if let Some(gamepads) = self.gamepads.clone() {
            gamepads.borrow_mut().poll(&mut self.input);
        }
    }
}

/// Map a UI key to the input crate's bindable key, or `None` when the key
/// cannot be bound to a joypad button (function keys, editing keys, …).
fn to_input_key(key: Key) -> Option<cgb_libretro::Key> {
    use cgb_libretro::Key as Input;
    Some(match key {
        Key::Character(c) => Input::Character(c),
        Key::ArrowUp => Input::ArrowUp,
        Key::ArrowDown => Input::ArrowDown,
        Key::ArrowLeft => Input::ArrowLeft,
        Key::ArrowRight => Input::ArrowRight,
        Key::Enter => Input::Enter,
        Key::Space => Input::Space,
        Key::Tab => Input::Tab,
        _ => return None,
    })
}
