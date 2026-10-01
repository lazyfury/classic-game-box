//! The UI: igui views built from a pure [`ViewModel`].
//!
//! This `ui` module is the only window-shaped part of the app, and it still
//! does not know about libretro or the platform. The app projects core state into a
//! [`ViewModel`], builds the tree, drains [`Action`]s, and never reaches into
//! the tree from a callback.
//!
//! See `docs/architecture/quill-native-migration.md` §7 and the igui
//! `docs/ui-guide.md` for the frame loop this wraps.

mod color;
mod focus;
mod frame;
mod icons;
mod model;
mod theme;
mod view;

pub use frame::{centered_fit, contain_fit, crop_fit, FrameImage};
pub use icons::{clear_textures, rasterize_icon, set_texture, Icon, IconName};
pub use model::{
    save_slot_label, Action, BindingRow, CatalogRow, CheatRow, Confirm, CoreOptionRow, CoreRow,
    EditTarget, GameRow, InputDescriptorRow, MissingCoreRow, MsaaKind, SafeArea, SaveSlotRow,
    ScreenshotRow, Section, SettingsGroup, ShaderKind, SortKey, StatusKind, SystemCount,
    TextureHandle, ViewModel,
};
pub use theme::{game_theme, ThemeChoice};
pub use view::{
    grid_window, library_columns, ViewBridge, CONTENT_DEFAULT_WIDTH, CONTENT_MAX_WIDTH,
    CONTENT_MIN_WIDTH, MAX_LIBRARY_COLUMNS, MIN_LIBRARY_COLUMNS,
};

use igui::igui_components::{NodeRef, OverlayId, Overlays, ScrollViewState};
use igui::igui_core::{Cursor, InputEvent, NodeId, Vec2, ViewportSize};
use igui::igui_render::PaintContext;
use igui::igui_scene::SceneTree;
use igui::igui_theme::Theme;
use igui::igui_ui::{hovered_cursor, Control, DragPhase, TextMeasurer};
use std::cell::RefCell;
use std::rc::Rc;

use cgb_libretro::SystemId;

/// The mounted tree plus the frame-loop calls.
pub struct Ui {
    tree: SceneTree,
    /// The active theme, kept so the focus ring can read `focus_ring`.
    theme: &'static dyn Theme,
    /// The overlay layer (tooltips, context menus, core pickers). It owns its
    /// own tree and paints on top of the UI.
    overlays: Overlays,
    /// Tooltips the current tree registered (`control → text`), resolved to
    /// mounted controls after the build.
    tips: Vec<(NodeId, String)>,
    /// The tip currently open, and the control it explains.
    open_tip: Option<(NodeId, OverlayId)>,
    /// The middle column's scroll state (the library grid, the screenshots
    /// grid or the settings bodies); `None` when the page has nothing to
    /// scroll.
    content_scroll: Option<ScrollViewState>,
    /// Which page `content_scroll` belongs to. A rebuild only carries the
    /// offset over while the page is unchanged.
    content_section: Section,
    /// The settings group `content_scroll` belongs to (the settings page's
    /// scroll lives in the right column). A group switch resets to the top.
    content_settings_group: SettingsGroup,
    /// The offset to restore on the next layout, set by a rebuild. It is
    /// applied *after* the first sync, once the content height is known.
    scroll_target: Option<f32>,
    /// Set when the tree changed and a layout + paint is needed before the
    /// next present. A running game redraws every frame; if nothing changed,
    /// the host can re-submit the previous draw list instead of rebuilding it.
    repaint: bool,
    /// The middle column's live width, shared with the resize handle. It lives
    /// here so a rebuild does not reset a width the user dragged.
    mount: view::ViewMount,
}

impl Ui {
    /// Build a fresh tree from the model.
    pub fn new(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Self {
        let mount = view::ViewMount::new(model.content_width);
        let (tree, content_scroll) = view::build(theme, model, actions, &mount);
        let tips = resolve_tips(actions.take_tips());
        Self {
            tree,
            theme,
            overlays: Overlays::new(theme),
            tips,
            open_tip: None,
            content_scroll,
            content_section: model.section,
            content_settings_group: model.settings_group,
            scroll_target: None,
            repaint: true,
            mount,
        }
    }

    /// Replace the tree with a rebuilt one (call when the model changed).
    ///
    /// A rebuild makes a fresh [`ScrollViewState`], so the offset would reset
    /// to the top — clicking a card (to play, pin or delete) would jump the
    /// grid. Carry the offset over when the page has not changed; it is
    /// restored after the next layout, once the new content height is known.
    ///
    /// A rebuild also drops the tree's pointer capture, so a drag in progress
    /// on the resize handle is re-armed on the new tree. That keeps the drag
    /// alive when a rebuild is triggered mid-drag by a grid column change.
    pub fn rebuild(&mut self, theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) {
        let same_page = self.content_section == model.section
            && self.content_settings_group == model.settings_group;
        let previous = same_page
            .then(|| self.content_scroll.as_ref().map(ScrollViewState::offset))
            .flatten();
        let dragging = igui::igui_ui::gui_state_of(&self.tree).and_then(|state| state.dragging);
        let drag_last = igui::igui_ui::gui_state_of(&self.tree).map(|state| state.drag_last);
        let was_resizing = dragging.is_some() && dragging == self.mount.resize_handle.get();

        let (tree, content_scroll) = view::build(theme, model, actions, &self.mount);
        let retheme = !std::ptr::eq(self.theme, theme);
        self.tree = tree;
        self.theme = theme;
        // Overlays hold the theme they were built with, so a theme switch (the
        // settings page) rebuilds them; a plain rebuild keeps them.
        if retheme {
            self.overlays = Overlays::new(theme);
        }
        // NodeIds from the old tree are dead, so any anchored overlay (tip or
        // menu) would point at nothing; close them and re-collect tooltips.
        self.overlays.close_all();
        self.open_tip = None;
        self.tips = resolve_tips(actions.take_tips());
        self.scroll_target = if content_scroll.is_some() {
            previous
        } else {
            None
        };
        self.content_scroll = content_scroll;
        self.content_section = model.section;
        self.content_settings_group = model.settings_group;
        self.repaint = true;
        if was_resizing {
            self.rearm_resize_drag(drag_last);
        }
    }

    /// Re-establish a resize drag on the freshly built handle, so a rebuild
    /// that happened mid-drag does not require the user to grab it again.
    fn rearm_resize_drag(&mut self, drag_last: Option<Vec2>) {
        let Some(id) = self.mount.resize_handle.get() else {
            return;
        };
        // Fire `Start` so the handle's own drag state (the grab cursor) matches.
        if let Some(callback) = self
            .tree
            .data::<Control>(id)
            .and_then(|control| control.drag_callback.clone())
        {
            (callback.borrow_mut())(&mut self.tree, id, DragPhase::Start, Vec2::ZERO);
        }
        let state = igui::igui_ui::gui_state_mut(&mut self.tree);
        state.dragging = Some(id);
        state.pressed = Some(id);
        state.focused = Some(id);
        if let Some(last) = drag_last {
            state.drag_last = last;
        }
    }

    /// Ask for a layout + paint before the next present (e.g. the viewport
    /// changed on a resize).
    pub fn request_repaint(&mut self) {
        self.repaint = true;
    }

    /// Take the pending repaint flag.
    pub fn take_repaint(&mut self) -> bool {
        std::mem::take(&mut self.repaint)
    }

    /// Update the play column's info text (FPS / resolution) in place, so a
    /// live readout does not rebuild the tree. A no-op before the line mounts.
    pub fn set_info(&mut self, info: &str) {
        let Some(id) = self.mount.info_label.get() else {
            return;
        };
        if igui::igui_components::set_text(&mut self.tree, id, info) {
            self.repaint = true;
        }
    }

    /// The middle column's live width, for the host to persist after a drag.
    pub fn content_width(&self) -> f32 {
        self.mount.content_width.get()
    }

    /// Whether the virtualized grid's mounted rows still cover `model`'s
    /// current scroll offset and viewport.
    ///
    /// The grid mounts only the rows around the viewport (see [`grid_window`]).
    /// While a scroll stays inside that window the tree can be left as is — the
    /// [`ScrollView`](igui::igui_components::ScrollView) just moves the mounted
    /// content — so the host should only rebuild when this returns `false`.
    pub fn grid_window_covers(&self, model: &ViewModel) -> bool {
        grid_window(model) == self.mount.mounted_rows.get()
    }

    /// The middle column's scroll offset, for the host to feed back into the
    /// model so the view can mount only the visible rows.
    pub fn scroll_offset(&self) -> f32 {
        self.content_scroll
            .as_ref()
            .map(ScrollViewState::offset)
            .unwrap_or(0.0)
    }

    /// The middle column's viewport height, or `0` before the first layout.
    pub fn scroll_viewport(&self) -> f32 {
        self.content_scroll
            .as_ref()
            .map(ScrollViewState::viewport_height)
            .unwrap_or(0.0)
    }

    /// The mounted tree.
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// The mounted tree, mutable.
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        &mut self.tree
    }

    /// Install the backend's real font metrics so layout measures what is
    /// painted. Call after [`Ui::new`] and after every [`Ui::rebuild`].
    pub fn install_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui::igui_ui::set_text_measurer(&mut self.tree, measurer.clone());
        self.overlays.set_text_measurer(measurer);
    }

    /// Install the host clipboard so the text fields can copy / cut / paste.
    /// Call after [`Ui::new`] and after every [`Ui::rebuild`] (a rebuild makes a
    /// fresh tree that does not inherit it).
    pub fn install_clipboard(&mut self, clipboard: Rc<RefCell<dyn igui::igui_ui::Clipboard>>) {
        igui::igui_ui::set_clipboard(&mut self.tree, clipboard.clone());
        self.overlays.set_clipboard(clipboard);
    }

    /// Resolve geometry and flush deferred tree work.
    ///
    /// A [`ScrollView`](igui::igui_components::ScrollView) resolves its viewport and
    /// content only after layout, so `sync` runs here and, when the offset
    /// moved the content, layout runs once more before paint. A pending
    /// [`Ui::rebuild`] scroll target is applied between the two syncs, because
    /// it can only be clamped once the content height is known.
    pub fn layout(&mut self, viewport: ViewportSize) {
        igui::igui_ui::layout(&mut self.tree, viewport);
        self.tree.update();
        if let Some(scroll) = self.content_scroll.as_mut() {
            let mut changed = scroll.sync(&mut self.tree);
            if let Some(target) = self.scroll_target.take() {
                scroll.scroll_to(target);
                changed |= scroll.sync(&mut self.tree);
            }
            if changed {
                igui::igui_ui::layout(&mut self.tree, viewport);
                self.tree.update();
            }
        }
        // Tooltips are anchored to laid-out controls, so open them after the
        // host layout and let the overlay layer position them.
        self.sync_tip();
        self.overlays.layout(&self.tree, viewport);
    }

    /// Emit this frame's draw list into `ctx`.
    pub fn paint(&self, ctx: &mut PaintContext) {
        igui::igui_ui::paint(&self.tree, ctx);
        self.paint_focus_ring(ctx);
        self.overlays.paint(ctx);
    }

    /// Stroke a ring around the focused control, so keyboard navigation is
    /// visible. Components do not draw one themselves (their backgrounds only
    /// read hover / press), so the host paints it once from the resolved rect.
    fn paint_focus_ring(&self, ctx: &mut PaintContext) {
        let Some(id) = igui::igui_ui::focused(&self.tree) else {
            return;
        };
        let Some(control) = igui::igui_ui::control(&self.tree, id) else {
            return;
        };
        if control.disabled || matches!(control.clip_rect, Some(rect) if rect.is_empty()) {
            return;
        }
        ctx.stroke_rect(control.rect, 2.0, self.theme.palette().focus_ring);
    }

    /// Advance overlay timers (message auto-dismiss, fades).
    pub fn update(&mut self, dt: f32) {
        self.overlays.update(dt);
    }

    /// Whether the overlay layer still needs frames (an open message counting
    /// down, a fade). The host schedules another redraw while this is true.
    pub fn overlays_animating(&self) -> bool {
        self.overlays.is_animating()
    }

    /// Close every open overlay. Called after an action so a menu does not
    /// linger once the user acted on it.
    pub fn close_overlays(&mut self) {
        self.overlays.close_all();
        self.open_tip = None;
        self.repaint = true;
    }

    /// Open a game card's context menu at `position` (a right click).
    pub fn open_game_menu(
        &mut self,
        theme: &'static dyn Theme,
        game: &GameRow,
        index: usize,
        position: Vec2,
        actions: &ViewBridge,
    ) {
        view::menus::game_menu(theme, &mut self.overlays, game, index, position, actions);
        self.repaint = true;
    }

    /// Open a console's core picker, anchored below its `Select` trigger.
    pub fn open_core_menu(
        &mut self,
        theme: &'static dyn Theme,
        system: SystemId,
        anchor: NodeId,
        cores: &[CoreRow],
        actions: &ViewBridge,
    ) {
        view::menus::core_menu(theme, &mut self.overlays, system, anchor, cores, actions);
        self.repaint = true;
    }

    /// The cursor the pointer should show, based on the control it is over.
    /// The host hands this to the window each frame (`AppLogic::cursor`); a
    /// control that is clickable reports [`Cursor::Pointer`].
    pub fn cursor(&self) -> Option<Cursor> {
        Some(hovered_cursor(&self.tree))
    }

    /// Open the per-game console picker: every console the app knows, with the
    /// current one ticked. Picking one overrides the system the file extension
    /// suggests (a `.chd` can be a PlayStation or a PSP disc).
    pub fn open_system_menu(
        &mut self,
        theme: &'static dyn Theme,
        game_id: i64,
        current: SystemId,
        position: Vec2,
        actions: &ViewBridge,
    ) {
        view::menus::system_menu(
            theme,
            &mut self.overlays,
            game_id,
            current,
            position,
            actions,
        );
        self.repaint = true;
    }

    /// Open the per-game core picker: the cores that run the game's console,
    /// with the game's own pick ticked (`game.core` is `None` when it follows
    /// the console's pick).
    pub fn open_game_core_menu(
        &mut self,
        theme: &'static dyn Theme,
        game: &GameRow,
        position: Vec2,
        cores: &[CoreRow],
        actions: &ViewBridge,
    ) {
        view::menus::game_core_menu(theme, &mut self.overlays, game, position, cores, actions);
        self.repaint = true;
    }

    /// Open a modal confirmation for a destructive action. Confirming runs
    /// `on_confirm`; Escape / clicking outside cancels.
    pub fn confirm_destructive(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: Action,
        actions: &ViewBridge,
    ) {
        view::menus::confirm_destructive(
            &mut self.overlays,
            title.into(),
            message.into(),
            on_confirm,
            actions,
        );
        self.repaint = true;
    }

    /// Open a modal confirmation that is not destructive (e.g. offering to
    /// download a core). Confirming runs `on_confirm`; Escape / clicking
    /// outside cancels.
    pub fn confirm(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: Action,
        actions: &ViewBridge,
    ) {
        view::menus::confirm_action(
            &mut self.overlays,
            title.into(),
            message.into(),
            on_confirm,
            actions,
        );
        self.repaint = true;
    }

    /// Open the tooltip the hovered control registered, or close the current
    /// one when the pointer left it.
    fn sync_tip(&mut self) {
        let wanted = igui::igui_ui::hovered(&self.tree).and_then(|node| self.tip_for(node));
        let unchanged = match (&self.open_tip, &wanted) {
            (Some((node, _)), Some((wanted, _))) => node == wanted,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        if let Some((_, id)) = self.open_tip.take() {
            self.overlays.close(id);
        }
        if let Some((node, text)) = wanted {
            let id = self.overlays.tips(node, text);
            self.open_tip = Some((node, id));
        }
    }

    /// The tooltip registered for `node` or its nearest registered ancestor.
    /// The hovered node is often a child of the button's root.
    fn tip_for(&self, node: NodeId) -> Option<(NodeId, String)> {
        let mut current = Some(node);
        while let Some(id) = current {
            if let Some((_, text)) = self.tips.iter().find(|(tip, _)| *tip == id) {
                return Some((id, text.clone()));
            }
            current = self.tree.parent(id);
        }
        None
    }

    /// Route one input event to the overlay layer only. The app calls this
    /// first so a menu can consume Escape even while a game captures the
    /// keyboard. Returns `true` when an overlay consumed it.
    pub fn route_overlay_input(&mut self, event: &InputEvent) -> bool {
        self.repaint = true;
        self.overlays.handle_input(event).is_handled()
    }

    /// Route one input event to the main UI (overlays excluded). Returns
    /// whether a control handled it.
    pub fn route_ui_input(&mut self, event: &InputEvent) -> bool {
        self.repaint = true;
        let handled = igui::igui_ui::route_input(&mut self.tree, event).is_handled();
        self.normalize_focus();
        handled
    }

    /// Route one input event: overlays first, then the UI. Returns `true` when
    /// the event was consumed (the app then skips the game bindings).
    pub fn route_input(&mut self, event: &InputEvent) -> bool {
        if self.route_overlay_input(event) {
            return true;
        }
        self.route_ui_input(event)
    }

    /// Move keyboard focus to the next (or previous) interactive control, in
    /// tree order. Returns `false` when the UI has nothing to focus.
    pub fn move_focus(&mut self, backward: bool) -> bool {
        let moved = focus::move_focus(&mut self.tree, backward);
        self.repaint |= moved;
        moved
    }

    /// Activate the focused control the way Enter / Space should: run its
    /// click callback. Returns `false` when nothing focused owns a click.
    pub fn activate_focus(&mut self) -> bool {
        let fired = focus::activate_focus(&mut self.tree);
        self.repaint |= fired;
        fired
    }

    /// Keep keyboard focus on a control that is focusable in its own right (a
    /// text field); a click on a label snaps up to its clickable ancestor so
    /// the ring and Tab agree.
    fn normalize_focus(&mut self) {
        focus::normalize(&mut self.tree);
    }
}

/// Resolve the view's `control → tooltip` refs to the mounted ids.
fn resolve_tips(raw: Vec<(NodeRef, String)>) -> Vec<(NodeId, String)> {
    raw.into_iter()
        .filter_map(|(node, text)| node.get().map(|id| (id, text)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgb_libretro::SystemId;
    use igui::igui_core::{InputEvent, PointerButton, Size, Vec2};
    use igui::igui_render::DrawCommand;
    use igui::igui_theme::{default_theme, Mode};

    fn game(index: usize) -> GameRow {
        GameRow {
            id: index as i64,
            name: format!("Game {index}"),
            file_name: format!("game{index}.nes"),
            system: SystemId::Nes,
            core: None,
            path: format!("/roms/game{index}.nes"),
            size: 0,
            pinned: false,
            play_count: 0,
            play_seconds: 0,
            last_played_at: 0,
            tags: Vec::new(),
            screenshots: 0,
            cover: None,
        }
    }

    /// A `TextInput` keeps keyboard focus while the UI routes other input; the
    /// app funnels every event through [`Ui::route_ui_input`].
    #[test]
    fn a_text_field_keeps_focus_and_accepts_text() {
        let theme = game_theme(Mode::Dark);
        let actions = ViewBridge::default();
        actions.set_edit(Rc::new(std::cell::RefCell::new(
            igui::igui_ui::TextEdit::new("ab"),
        )));
        let model = ViewModel {
            editing: Some(EditTarget::GameName(0)),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        let focused = igui::igui_ui::focused(ui.tree()).expect("the field autofocuses");

        ui.route_ui_input(&InputEvent::PointerMove {
            position: Vec2::new(10.0, 10.0),
        });
        assert_eq!(
            igui::igui_ui::focused(ui.tree()),
            Some(focused),
            "focus stays on the field after routing input"
        );

        ui.route_ui_input(&InputEvent::TextInput { text: "c".into() });
        assert_eq!(actions.edit_text(), "abc");
    }

    /// The focused field reads its modifiers from the tree, so the host must
    /// route `ModifiersChanged` before the key. Swallowing it (the old bug)
    /// turned Ctrl+A into a plain `a`.
    #[test]
    fn a_text_field_sees_routed_modifiers_for_shortcuts() {
        use igui::igui_core::{Key, Modifiers};

        let theme = game_theme(Mode::Dark);
        let actions = ViewBridge::default();
        actions.set_edit(Rc::new(std::cell::RefCell::new(
            igui::igui_ui::TextEdit::new("abc"),
        )));
        let model = ViewModel {
            editing: Some(EditTarget::GameName(0)),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);

        ui.route_ui_input(&InputEvent::ModifiersChanged(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }));
        ui.route_ui_input(&InputEvent::KeyDown {
            key: Key::Character('a'),
        });
        // Select-all, then typing replaces the whole value.
        ui.route_ui_input(&InputEvent::TextInput { text: "z".into() });
        assert_eq!(actions.edit_text(), "z");
    }

    /// Copy / paste only work once a clipboard is installed on the tree; the
    /// host must read the platform service and call [`Ui::install_clipboard`].
    #[test]
    fn a_text_field_copies_and_pastes_through_the_clipboard() {
        use igui::igui_core::{Key, Modifiers};
        use igui::igui_ui::MemoryClipboard;

        let theme = game_theme(Mode::Dark);
        let actions = ViewBridge::default();
        actions.set_edit(Rc::new(std::cell::RefCell::new(
            igui::igui_ui::TextEdit::new("abc"),
        )));
        let model = ViewModel {
            editing: Some(EditTarget::GameName(0)),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.install_clipboard(Rc::new(std::cell::RefCell::new(MemoryClipboard::default())));
        ui.layout(viewport);

        ui.route_ui_input(&InputEvent::ModifiersChanged(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }));
        // Select all, copy, overwrite, then paste the copied value back.
        ui.route_ui_input(&InputEvent::KeyDown {
            key: Key::Character('a'),
        });
        ui.route_ui_input(&InputEvent::KeyDown {
            key: Key::Character('c'),
        });
        ui.route_ui_input(&InputEvent::TextInput { text: "z".into() });
        ui.route_ui_input(&InputEvent::KeyDown {
            key: Key::Character('v'),
        });
        assert_eq!(actions.edit_text(), "zabc");
    }

    /// A rebuild (a card click rebuilds the tree) must not jump the grid back
    /// to the top.
    #[test]
    fn a_rebuild_keeps_the_scroll_offset() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            games: (0..40).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.content_scroll
            .as_ref()
            .expect("the library scrolls")
            .scroll_to(120.0);
        ui.layout(viewport);
        let before = ui.content_scroll.as_ref().unwrap().offset();
        assert!(before > 0.0, "the grid scrolled: {before}");

        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        let after = ui.content_scroll.as_ref().unwrap().offset();
        assert_eq!(after, before, "the offset survives the rebuild");
    }

    /// Switching the theme at runtime rebuilds with the new palette (and its
    /// overlays) rather than keeping the old one.
    #[test]
    fn a_retheme_swaps_the_active_theme() {
        let actions = ViewBridge::default();
        let model = ViewModel::default();
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(default_theme(Mode::Dark), &model, &actions);
        ui.layout(viewport);

        ui.rebuild(game_theme(Mode::Dark), &model, &actions);
        ui.layout(viewport);
        assert_eq!(
            ui.theme.palette().accent.to_rgba8(),
            [0xFF, 0xB0, 0x20, 0xFF],
            "the custom accent is active after the switch"
        );
    }

    /// Switching pages must not carry the library's offset into the settings
    /// page.
    #[test]
    fn switching_pages_does_not_carry_the_offset() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let mut model = ViewModel {
            games: (0..40).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.content_scroll.as_ref().unwrap().scroll_to(120.0);
        ui.layout(viewport);
        assert!(ui.content_scroll.as_ref().unwrap().offset() > 0.0);

        model.section = Section::Settings;
        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        assert_eq!(
            ui.content_scroll.as_ref().map(ScrollViewState::offset),
            Some(0.0),
            "the settings page starts at the top"
        );
    }

    /// Switching settings groups resets the detail scroll to the top, even
    /// though the section (and so the scroll state) is the same.
    #[test]
    fn switching_settings_groups_resets_the_offset() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let mut model = ViewModel {
            section: Section::Settings,
            settings_group: SettingsGroup::Input,
            bindings: (0..40)
                .map(|index| BindingRow {
                    button: format!("按键 {index}"),
                    keys: "X / K".to_string(),
                })
                .collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.content_scroll
            .as_ref()
            .expect("the settings detail scrolls")
            .scroll_to(120.0);
        ui.layout(viewport);
        assert!(ui.content_scroll.as_ref().unwrap().offset() > 0.0);

        model.settings_group = SettingsGroup::Display;
        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        assert_eq!(
            ui.content_scroll.as_ref().map(ScrollViewState::offset),
            Some(0.0),
            "a new group starts at the top"
        );
    }

    /// A small scroll stays inside the mounted rows, so the host can repaint
    /// without rebuilding; scrolling past them asks for a rebuild.
    #[test]
    fn a_small_scroll_stays_inside_the_mounted_window() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let mut model = ViewModel {
            games: (0..40).map(game).collect(),
            grid_viewport: 400.0,
            ..ViewModel::default()
        };
        let ui = Ui::new(theme, &model, &actions);

        // A few pixels into the first row: still covered.
        model.grid_offset = 16.0;
        assert!(
            ui.grid_window_covers(&model),
            "a sub-row scroll needs no rebuild"
        );

        // Far past the mounted rows: a rebuild is required.
        model.grid_offset = 4000.0;
        assert!(
            !ui.grid_window_covers(&model),
            "scrolling past the mounted rows needs a rebuild"
        );
    }

    /// Dragging the divider widens the middle column, and the width survives a
    /// rebuild (the app reads it back to persist the split).
    #[test]
    fn dragging_the_divider_resizes_the_middle_column() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel::default();
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        // The 6px gutter sits at the middle column's right edge.
        let start = Vec2::new(64.0 + model.content_width + 3.0, 400.0);
        let end = start + Vec2::new(40.0, 0.0);
        ui.route_input(&InputEvent::PointerDown {
            position: start,
            button: PointerButton::Left,
        });
        ui.route_input(&InputEvent::PointerMove { position: end });
        ui.route_input(&InputEvent::PointerUp {
            position: end,
            button: PointerButton::Left,
        });
        ui.layout(viewport);
        let widened = model.content_width + 40.0;
        assert!(
            (ui.content_width() - widened).abs() < 1e-3,
            "the drag widened the column: {}",
            ui.content_width()
        );

        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        assert!(
            (ui.content_width() - widened).abs() < 1e-3,
            "the width survives a rebuild: {}",
            ui.content_width()
        );
    }

    /// A rebuild mid-drag (the throttled grid column change) must not drop the
    /// pointer capture: the next move keeps resizing.
    #[test]
    fn a_rebuild_mid_drag_keeps_resizing() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel::default();
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        let start = Vec2::new(64.0 + model.content_width + 3.0, 400.0);
        ui.route_input(&InputEvent::PointerDown {
            position: start,
            button: PointerButton::Left,
        });
        ui.route_input(&InputEvent::PointerMove {
            position: start + Vec2::new(20.0, 0.0),
        });
        // A column-count change rebuilds the tree while the pointer is down.
        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        let end = start + Vec2::new(60.0, 0.0);
        ui.route_input(&InputEvent::PointerMove { position: end });
        ui.route_input(&InputEvent::PointerUp {
            position: end,
            button: PointerButton::Left,
        });
        assert!(
            (ui.content_width() - (model.content_width + 60.0)).abs() < 1e-3,
            "the drag continued after the rebuild: {}",
            ui.content_width()
        );
    }

    /// Tab moves the focus to the next interactive control, Shift+Tab back.
    #[test]
    fn tab_moves_the_focus_between_controls() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            games: (0..3).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        assert!(
            igui::igui_ui::focused(ui.tree()).is_none(),
            "nothing is focused until the keyboard is used"
        );

        assert!(ui.move_focus(false), "a control takes focus");
        let first = igui::igui_ui::focused(ui.tree()).expect("focused");
        assert!(ui.move_focus(false), "focus advances");
        let second = igui::igui_ui::focused(ui.tree()).expect("focused");
        assert_ne!(first, second, "Tab moved the focus to another control");

        assert!(ui.move_focus(true), "focus goes back");
        assert_eq!(igui::igui_ui::focused(ui.tree()), Some(first));
    }

    /// Enter / Space on the focused control runs its click callback.
    #[test]
    fn activating_the_focus_fires_its_click() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            games: (0..3).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        assert!(ui.move_focus(false), "a control takes focus");
        assert!(ui.activate_focus(), "the focused control fires its click");
        assert!(
            !actions.drain().is_empty(),
            "activating the focus pushed an action"
        );
    }

    /// The focused control is stroked with a focus ring when painted.
    #[test]
    fn the_focused_control_gets_a_ring() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            games: (0..3).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.move_focus(false);

        let mut ctx = igui::igui_render::PaintContext::new();
        ui.paint(&mut ctx);
        let list = ctx.into_draw_list();
        assert!(
            list.commands().iter().any(|command| matches!(
                command,
                igui::igui_render::DrawCommand::StrokeRect { .. }
            )),
            "the focus ring is stroked"
        );
    }

    /// A click on a card's text focuses the card (the click target), not the
    /// label, so the ring and Tab agree.
    #[test]
    fn clicking_card_text_focuses_the_card() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            games: (0..3).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        let mut ctx = igui::igui_render::PaintContext::new();
        ui.paint(&mut ctx);
        let list = ctx.into_draw_list();
        let point = list
            .commands()
            .iter()
            .find_map(|command| match command {
                igui::igui_render::DrawCommand::DrawText { text, position, .. }
                    if text == "Game 0" =>
                {
                    Some(*position)
                }
                _ => None,
            })
            .expect("the name is painted");

        for event in [
            InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
            InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        ] {
            ui.route_input(&event);
        }

        let focused = igui::igui_ui::focused(ui.tree()).expect("something is focused");
        let control = ui.tree().data::<Control>(focused).expect("a control");
        assert!(
            control.callback.is_some(),
            "the focus snapped to the click target, not the label"
        );

        // Tab continues from that control instead of restarting.
        assert!(ui.move_focus(false));
        assert_ne!(igui::igui_ui::focused(ui.tree()), Some(focused));
    }

    fn painted(ui: &Ui) -> igui::igui_render::DrawList {
        let mut ctx = PaintContext::new();
        ui.paint(&mut ctx);
        ctx.into_draw_list()
    }

    fn painted_text(ui: &Ui) -> Vec<String> {
        painted(ui)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_position(ui: &Ui, needle: &str) -> Vec2 {
        painted(ui)
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no painted text {needle:?}"))
    }

    /// A card's context menu offers「选择核心…」, and the picker lists the
    /// console's cores with the game's own choice ticked. Clicking one records
    /// the pick as an action.
    #[test]
    fn the_card_menu_opens_a_per_game_core_picker() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let cores = vec![
            CoreRow {
                key: "mesen".to_string(),
                name: "Mesen".to_string(),
                system: SystemId::Nes,
                selected: true,
            },
            CoreRow {
                key: "nestopia".to_string(),
                name: "Nestopia".to_string(),
                system: SystemId::Nes,
                selected: false,
            },
            CoreRow {
                key: "mgba".to_string(),
                name: "mGBA".to_string(),
                system: SystemId::Gba,
                selected: false,
            },
        ];
        let mut game = game(0);
        game.core = Some("nestopia".to_string());
        let model = ViewModel {
            section: Section::Library,
            games: vec![game.clone()],
            cores,
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);

        // The card menu carries the entry.
        ui.open_game_menu(theme, &game, 0, Vec2::new(120.0, 120.0), &actions);
        ui.layout(viewport);
        let texts = painted_text(&ui);
        assert!(
            texts.iter().any(|text| text == "选择核心…"),
            "the card menu offers the picker: {texts:?}"
        );

        // The picker filters to the console and ticks the game's own pick.
        ui.close_overlays();
        ui.open_game_core_menu(
            theme,
            &game,
            Vec2::new(120.0, 120.0),
            &model.cores,
            &actions,
        );
        ui.layout(viewport);
        let texts = painted_text(&ui);
        assert!(texts.iter().any(|text| text == "Nestopia ✓"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "Mesen"), "{texts:?}");
        assert!(
            texts.iter().any(|text| text == "默认（跟随机种设置）"),
            "the override can be cleared: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "mGBA"),
            "another console's core is not offered: {texts:?}"
        );

        // Clicking a core asks the host to remember it.
        let point = text_position(&ui, "Mesen");
        for event in [
            InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
            InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        ] {
            ui.route_input(&event);
        }
        assert_eq!(
            actions.drain(),
            vec![Action::SetGameCore {
                id: 0,
                core: Some("mesen".to_string()),
            }]
        );
    }

    /// A game that follows its console's pick ticks the clear-override entry.
    #[test]
    fn a_game_with_no_core_pick_ticks_the_console_default() {
        let theme = default_theme(Mode::Dark);
        let actions = ViewBridge::default();
        let model = ViewModel {
            section: Section::Library,
            games: vec![game(0)],
            cores: vec![CoreRow {
                key: "nestopia".to_string(),
                name: "Nestopia".to_string(),
                system: SystemId::Nes,
                selected: false,
            }],
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
        let mut ui = Ui::new(theme, &model, &actions);
        ui.open_game_core_menu(
            theme,
            &model.games[0],
            Vec2::new(120.0, 120.0),
            &model.cores,
            &actions,
        );
        ui.layout(viewport);
        let texts = painted_text(&ui);
        assert!(
            texts.iter().any(|text| text == "默认（跟随机种设置）✓"),
            "the console default is ticked: {texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Nestopia"), "{texts:?}");
    }
}
