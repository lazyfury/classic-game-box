//! The UI: quill views built from a pure [`ViewModel`].
//!
//! `cgb-ui` is the only crate with a window-shaped API, and it still does not
//! know about libretro or the platform. The app projects core state into a
//! [`ViewModel`], builds the tree, drains [`Action`]s, and never reaches into
//! the tree from a callback.
//!
//! See `docs/architecture/quill-native-migration.md` §7 and the quill
//! `docs/ui-guide.md` for the frame loop this wraps.

mod frame;
mod icons;
mod model;
mod theme;
mod view;

pub use frame::{centered_fit, contain_fit, cover_fit, FrameImage};
pub use icons::{clear_textures, rasterize_icon, set_texture, Icon, IconName};
pub use model::{
    Action, BindingRow, CheatRow, Confirm, CoreOptionRow, CoreRow, EditKind, EditState,
    FrameHandle, GameRow, InputDescriptorRow, MsaaKind, SafeArea, SaveSlotRow, ScreenshotRow,
    Section, ShaderKind, SortKey, StatusKind, SystemCount, ViewModel,
};
pub use theme::{game_theme, ThemeChoice};
pub use view::{
    grid_window, library_columns, Actions, MAX_LIBRARY_COLUMNS, MIDDLE_MAX_WIDTH, MIDDLE_MIN_WIDTH,
    MIN_LIBRARY_COLUMNS,
};

use std::cell::Cell;
use std::rc::Rc;

use igui::igui_components::{Menu, MenuItem, NodeRef, OverlayId, Overlays, ScrollViewState};
use igui::igui_core::{InputEvent, NodeId, Vec2, ViewportSize};
use igui::igui_render::PaintContext;
use igui::igui_scene::SceneTree;
use igui::igui_theme::Theme;
use igui::igui_ui::{Control, DragPhase, TextMeasurer};

use cgb_systems::{SystemId, SYSTEMS};

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
    middle_scroll: Option<ScrollViewState>,
    /// Which page `middle_scroll` belongs to. A rebuild only carries the
    /// offset over while the page is unchanged.
    middle_section: Section,
    /// The offset to restore on the next layout, set by a rebuild. It is
    /// applied *after* the first sync, once the content height is known.
    scroll_target: Option<f32>,
    /// Set when the tree changed and a layout + paint is needed before the
    /// next present. A running game redraws every frame; if nothing changed,
    /// the host can re-submit the previous draw list instead of rebuilding it.
    repaint: bool,
    /// The middle column's live width, shared with the resize handle. It lives
    /// here so a rebuild does not reset a width the user dragged.
    middle_width: Rc<Cell<f32>>,
    /// The resize handle's mounted node. A rebuild (e.g. a grid column-count
    /// change) drops the tree's pointer capture, so it is re-armed from here.
    resize_handle: NodeRef,
    /// The row range the active virtualized grid mounted in the current tree.
    /// The app uses it to skip a rebuild while scrolling inside it.
    mounted_rows: Cell<(usize, usize)>,
    /// The play column's live info label (FPS / resolution), updated in place.
    info_label: NodeRef,
}

impl Ui {
    /// Build a fresh tree from the model.
    pub fn new(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Self {
        let middle_width = Rc::new(Cell::new(
            model.middle_width.clamp(MIDDLE_MIN_WIDTH, MIDDLE_MAX_WIDTH),
        ));
        let resize_handle = NodeRef::new();
        let mounted_rows = Cell::new((0, 0));
        let info_label = NodeRef::new();
        let (tree, middle_scroll) = view::build(
            theme,
            model,
            actions,
            &middle_width,
            &resize_handle,
            &mounted_rows,
            &info_label,
        );
        let tips = resolve_tips(actions.take_tips());
        Self {
            tree,
            theme,
            overlays: Overlays::new(theme),
            tips,
            open_tip: None,
            middle_scroll,
            middle_section: model.section,
            scroll_target: None,
            repaint: true,
            middle_width,
            resize_handle,
            mounted_rows,
            info_label,
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
    pub fn rebuild(&mut self, theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) {
        let previous = (self.middle_section == model.section)
            .then(|| self.middle_scroll.as_ref().map(ScrollViewState::offset))
            .flatten();
        let dragging = igui::igui_ui::gui_state_of(&self.tree).and_then(|state| state.dragging);
        let drag_last = igui::igui_ui::gui_state_of(&self.tree).map(|state| state.drag_last);
        let was_resizing = dragging.is_some() && dragging == self.resize_handle.get();

        let (tree, middle_scroll) = view::build(
            theme,
            model,
            actions,
            &self.middle_width,
            &self.resize_handle,
            &self.mounted_rows,
            &self.info_label,
        );
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
        self.scroll_target = if middle_scroll.is_some() {
            previous
        } else {
            None
        };
        self.middle_scroll = middle_scroll;
        self.middle_section = model.section;
        self.repaint = true;
        if was_resizing {
            self.rearm_resize_drag(drag_last);
        }
    }

    /// Re-establish a resize drag on the freshly built handle, so a rebuild
    /// that happened mid-drag does not require the user to grab it again.
    fn rearm_resize_drag(&mut self, drag_last: Option<Vec2>) {
        let Some(id) = self.resize_handle.get() else {
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
        let Some(id) = self.info_label.get() else {
            return;
        };
        if igui::igui_components::set_text(&mut self.tree, id, info) {
            self.repaint = true;
        }
    }

    /// The middle column's live width, for the host to persist after a drag.
    pub fn middle_width(&self) -> f32 {
        self.middle_width.get()
    }

    /// Whether the virtualized grid's mounted rows still cover `model`'s
    /// current scroll offset and viewport.
    ///
    /// The grid mounts only the rows around the viewport (see [`grid_window`]).
    /// While a scroll stays inside that window the tree can be left as is — the
    /// [`ScrollView`](igui::igui_components::ScrollView) just moves the mounted
    /// content — so the host should only rebuild when this returns `false`.
    pub fn grid_window_covers(&self, model: &ViewModel) -> bool {
        grid_window(model) == self.mounted_rows.get()
    }

    /// The middle column's scroll offset, for the host to feed back into the
    /// model so the view can mount only the visible rows.
    pub fn scroll_offset(&self) -> f32 {
        self.middle_scroll
            .as_ref()
            .map(ScrollViewState::offset)
            .unwrap_or(0.0)
    }

    /// The middle column's viewport height, or `0` before the first layout.
    pub fn scroll_viewport(&self) -> f32 {
        self.middle_scroll
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
        if let Some(scroll) = self.middle_scroll.as_mut() {
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
        actions: &Actions,
    ) {
        let game_id = game.id;
        let pinned = game.pinned;
        let actions = actions.clone();
        self.overlays.menu_at(position, move |tree, node| {
            let item = |label: &str, action: Action| {
                let actions = actions.clone();
                MenuItem::new(label, theme).on_click(move |_tree, _id| actions.push(action))
            };
            let menu = Menu::new(theme)
                .item(item("开始游戏", Action::Play(index)))
                .item(item("改名…", Action::StartRename(game_id)))
                .item(item(
                    "选择机种…",
                    Action::OpenSystemMenu {
                        id: game_id,
                        position,
                    },
                ))
                .item(item("标签…", Action::StartTagEdit(game_id)))
                .item(item(
                    if pinned { "取消置顶" } else { "置顶" },
                    Action::TogglePin(index),
                ))
                .item(item("截图", Action::ShowScreenshots(game_id)))
                .separator()
                .item({
                    let actions = actions.clone();
                    MenuItem::new("删除…", theme)
                        .destructive()
                        .on_click(move |_tree, _id| {
                            actions.push(Action::RequestDelete(Confirm::DeleteGame(game_id)))
                        })
                });
            tree.add_child(node, menu);
        });
        self.repaint = true;
    }

    /// Open a console's core picker, anchored below its `Select` trigger.
    pub fn open_core_menu(
        &mut self,
        theme: &'static dyn Theme,
        system: SystemId,
        anchor: NodeId,
        cores: &[CoreRow],
        actions: &Actions,
    ) {
        let rows: Vec<(usize, String, bool)> = cores
            .iter()
            .enumerate()
            .filter(|(_, core)| core.system == system)
            .map(|(index, core)| (index, core.name.clone(), core.selected))
            .collect();
        let actions = actions.clone();
        self.overlays.menu(anchor, move |tree, node| {
            let mut menu = Menu::new(theme);
            for (index, name, selected) in rows.clone() {
                let label = if selected {
                    format!("{name} ✓")
                } else {
                    name
                };
                let actions = actions.clone();
                menu = menu.item(
                    MenuItem::new(label, theme)
                        .on_click(move |_tree, _id| actions.push(Action::SelectCore(index))),
                );
            }
            tree.add_child(node, menu);
        });
        self.repaint = true;
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
        actions: &Actions,
    ) {
        // Replaces the card menu this was opened from.
        self.overlays.close_all();
        let actions = actions.clone();
        self.overlays.menu_at(position, move |tree, node| {
            let mut menu = Menu::new(theme);
            for system in SYSTEMS {
                let label = if *system == current {
                    format!("{} ✓", system.name())
                } else {
                    system.name().to_string()
                };
                let actions = actions.clone();
                let system = *system;
                menu = menu.item(MenuItem::new(label, theme).on_click(move |_tree, _id| {
                    actions.push(Action::SetGameSystem {
                        id: game_id,
                        system,
                    })
                }));
            }
            tree.add_child(node, menu);
        });
        self.repaint = true;
    }

    /// Open a modal confirmation for a destructive action. Confirming runs
    /// `on_confirm`; Escape / clicking outside cancels.
    pub fn confirm_destructive(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: Action,
        actions: &Actions,
    ) {
        let id = self.overlays.confirm(title.into(), message.into());
        self.overlays.destructive(id, true);
        let actions = actions.clone();
        self.overlays
            .on_confirm(id, move || actions.push(on_confirm));
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
        let order = focus_order(&self.tree);
        if order.is_empty() {
            return false;
        }
        let current = igui::igui_ui::focused(&self.tree);
        let index = current.and_then(|id| order.iter().position(|node| *node == id));
        let next = match index {
            Some(index) if backward => (index + order.len() - 1) % order.len(),
            Some(index) => (index + 1) % order.len(),
            None if backward => order.len() - 1,
            None => 0,
        };
        igui::igui_ui::gui_state_mut(&mut self.tree).focused = Some(order[next]);
        self.repaint = true;
        true
    }

    /// Activate the focused control the way Enter / Space should: run its
    /// click callback. quill's own keyboard activation only fires for its
    /// low-level `Widget::Button`, which the themed components and this app's
    /// custom targets do not use, so the host drives it here.
    pub fn activate_focus(&mut self) -> bool {
        let Some(id) = igui::igui_ui::focused(&self.tree) else {
            return false;
        };
        // A disabled control (or one under a disabled ancestor) never fires.
        let mut guard = Some(id);
        while let Some(node) = guard {
            if self
                .tree
                .data::<Control>(node)
                .is_some_and(|control| control.data.disabled)
            {
                return false;
            }
            guard = self.tree.parent(node);
        }
        // The nearest ancestor with a callback owns the click.
        let mut current = Some(id);
        while let Some(node) = current {
            if let Some(callback) = self
                .tree
                .data::<Control>(node)
                .and_then(|control| control.callback.clone())
            {
                (callback.borrow_mut())(&mut self.tree, node);
                self.repaint = true;
                return true;
            }
            current = self.tree.parent(node);
        }
        false
    }

    /// A pointer click lands on the deepest control under it, which for a card
    /// or a row is a text label with no click action. Snap the focus to the
    /// nearest ancestor that has one (the card / button the label belongs to),
    /// or clear it when there is none, so the ring and Tab agree with what Tab
    /// would pick.
    /// Keep keyboard focus on a control that is focusable in its own right (a
    /// text field). The old heuristic walked every focused node up to the
    /// nearest click handler, which cleared a field's focus as soon as any
    /// input was routed.
    fn normalize_focus(&mut self) {
        let Some(id) = igui::igui_ui::focused(&self.tree) else {
            return;
        };
        if self
            .tree
            .data::<Control>(id)
            .is_some_and(|control| control.focusable)
        {
            return;
        }
        let target = interactive_ancestor(&self.tree, id);
        if target != Some(id) {
            igui::igui_ui::gui_state_mut(&mut self.tree).focused = target;
        }
    }
}

/// The nearest ancestor of `id` (including itself) with a click callback.
fn interactive_ancestor(tree: &SceneTree, id: NodeId) -> Option<NodeId> {
    let mut current = Some(id);
    while let Some(node) = current {
        if tree
            .data::<Control>(node)
            .is_some_and(|control| control.callback.is_some())
        {
            return Some(node);
        }
        current = tree.parent(node);
    }
    None
}

/// Every focusable control in tree order: the nodes that own a click or drag
/// callback, are enabled, and are not clipped away.
fn focus_order(tree: &SceneTree) -> Vec<NodeId> {
    let mut order = Vec::new();
    collect_focusable(tree, tree.root(), &mut order);
    order
}

fn collect_focusable(tree: &SceneTree, id: NodeId, out: &mut Vec<NodeId>) {
    if let Some(control) = tree.data::<Control>(id) {
        // Only controls a keyboard can activate: a click callback. (A drag
        // handle has no keyboard equivalent yet, so it is not a focus stop.)
        let interactive = control.callback.is_some();
        let clipped = matches!(control.data.clip_rect, Some(rect) if rect.is_empty());
        if interactive && !control.data.disabled && !clipped {
            out.push(id);
        }
    }
    if let Some(children) = tree.children(id) {
        for child in children {
            collect_focusable(tree, *child, out);
        }
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
    use cgb_systems::SystemId;
    use igui::igui_core::{InputEvent, PointerButton, Size, Vec2};
    use igui::igui_theme::{default_theme, Mode};

    fn game(index: usize) -> GameRow {
        GameRow {
            id: index as i64,
            name: format!("Game {index}"),
            file_name: format!("game{index}.nes"),
            system: SystemId::Nes,
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
        let actions = Actions::default();
        actions.set_edit(Rc::new(std::cell::RefCell::new(
            igui::igui_ui::TextEdit::new("ab"),
        )));
        let model = ViewModel {
            editing: Some(EditState {
                game_id: 0,
                kind: EditKind::Name,
            }),
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

    /// A rebuild (a card click rebuilds the tree) must not jump the grid back
    /// to the top.
    #[test]
    fn a_rebuild_keeps_the_scroll_offset() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel {
            games: (0..40).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.middle_scroll
            .as_ref()
            .expect("the library scrolls")
            .scroll_to(120.0);
        ui.layout(viewport);
        let before = ui.middle_scroll.as_ref().unwrap().offset();
        assert!(before > 0.0, "the grid scrolled: {before}");

        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        let after = ui.middle_scroll.as_ref().unwrap().offset();
        assert_eq!(after, before, "the offset survives the rebuild");
    }

    /// Switching the theme at runtime rebuilds with the new palette (and its
    /// overlays) rather than keeping the old one.
    #[test]
    fn a_retheme_swaps_the_active_theme() {
        let actions = Actions::default();
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
        let actions = Actions::default();
        let mut model = ViewModel {
            games: (0..40).map(game).collect(),
            ..ViewModel::default()
        };
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        ui.middle_scroll.as_ref().unwrap().scroll_to(120.0);
        ui.layout(viewport);
        assert!(ui.middle_scroll.as_ref().unwrap().offset() > 0.0);

        model.section = Section::Settings;
        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        assert_eq!(
            ui.middle_scroll.as_ref().map(ScrollViewState::offset),
            Some(0.0),
            "the settings page starts at the top"
        );
    }

    /// A small scroll stays inside the mounted rows, so the host can repaint
    /// without rebuilding; scrolling past them asks for a rebuild.
    #[test]
    fn a_small_scroll_stays_inside_the_mounted_window() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
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
        let actions = Actions::default();
        let model = ViewModel::default();
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        // The 6px gutter sits at the middle column's right edge.
        let start = Vec2::new(64.0 + model.middle_width + 3.0, 400.0);
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
        let widened = model.middle_width + 40.0;
        assert!(
            (ui.middle_width() - widened).abs() < 1e-3,
            "the drag widened the column: {}",
            ui.middle_width()
        );

        ui.rebuild(theme, &model, &actions);
        ui.layout(viewport);
        assert!(
            (ui.middle_width() - widened).abs() < 1e-3,
            "the width survives a rebuild: {}",
            ui.middle_width()
        );
    }

    /// A rebuild mid-drag (the throttled grid column change) must not drop the
    /// pointer capture: the next move keeps resizing.
    #[test]
    fn a_rebuild_mid_drag_keeps_resizing() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel::default();
        let viewport = ViewportSize::new(Size::new(1100.0, 760.0));

        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        let start = Vec2::new(64.0 + model.middle_width + 3.0, 400.0);
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
            (ui.middle_width() - (model.middle_width + 60.0)).abs() < 1e-3,
            "the drag continued after the rebuild: {}",
            ui.middle_width()
        );
    }

    /// Tab moves the focus to the next interactive control, Shift+Tab back.
    #[test]
    fn tab_moves_the_focus_between_controls() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
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
        let actions = Actions::default();
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
        let actions = Actions::default();
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
        let actions = Actions::default();
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
}
