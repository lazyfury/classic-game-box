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
mod view;

pub use frame::{centered_fit, contain_fit, cover_fit, FrameImage};
pub use icons::{clear_textures, rasterize_icon, set_texture, Icon, IconName};
pub use model::{
    Action, BindingRow, CheatRow, Confirm, CoreOptionRow, CoreRow, EditKind, EditState,
    FrameHandle, GameRow, InputDescriptorRow, SafeArea, SaveSlotRow, ScreenshotRow, Section,
    ShaderKind, SortKey, ViewModel,
};
pub use view::{
    grid_window, library_columns, Actions, MAX_LIBRARY_COLUMNS, MIDDLE_MAX_WIDTH, MIDDLE_MIN_WIDTH,
    MIN_LIBRARY_COLUMNS,
};

use std::cell::Cell;
use std::rc::Rc;

use draw_components::{NodeRef, ScrollViewState};
use draw_core::{InputEvent, Vec2, ViewportSize};
use draw_render::PaintContext;
use draw_scene::SceneTree;
use draw_theme::Theme;
use draw_ui::{Control, DragPhase, TextMeasurer};

/// The mounted tree plus the frame-loop calls.
pub struct Ui {
    tree: SceneTree,
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
}

impl Ui {
    /// Build a fresh tree from the model.
    pub fn new(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Self {
        let middle_width = Rc::new(Cell::new(
            model.middle_width.clamp(MIDDLE_MIN_WIDTH, MIDDLE_MAX_WIDTH),
        ));
        let resize_handle = NodeRef::new();
        let mounted_rows = Cell::new((0, 0));
        let (tree, middle_scroll) = view::build(
            theme,
            model,
            actions,
            &middle_width,
            &resize_handle,
            &mounted_rows,
        );
        Self {
            tree,
            middle_scroll,
            middle_section: model.section,
            scroll_target: None,
            repaint: true,
            middle_width,
            resize_handle,
            mounted_rows,
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
        let dragging = draw_ui::gui_state_of(&self.tree).and_then(|state| state.dragging);
        let drag_last = draw_ui::gui_state_of(&self.tree).map(|state| state.drag_last);
        let was_resizing = dragging.is_some() && dragging == self.resize_handle.get();

        let (tree, middle_scroll) = view::build(
            theme,
            model,
            actions,
            &self.middle_width,
            &self.resize_handle,
            &self.mounted_rows,
        );
        self.tree = tree;
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
            (callback.borrow_mut())(&mut self.tree, DragPhase::Start, Vec2::ZERO);
        }
        let state = draw_ui::gui_state_mut(&mut self.tree);
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

    /// The middle column's live width, for the host to persist after a drag.
    pub fn middle_width(&self) -> f32 {
        self.middle_width.get()
    }

    /// Whether the virtualized grid's mounted rows still cover `model`'s
    /// current scroll offset and viewport.
    ///
    /// The grid mounts only the rows around the viewport (see [`grid_window`]).
    /// While a scroll stays inside that window the tree can be left as is — the
    /// [`ScrollView`](draw_components::ScrollView) just moves the mounted
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
        draw_ui::set_text_measurer(&mut self.tree, measurer);
    }

    /// Resolve geometry and flush deferred tree work.
    ///
    /// A [`ScrollView`](draw_components::ScrollView) resolves its viewport and
    /// content only after layout, so `sync` runs here and, when the offset
    /// moved the content, layout runs once more before paint. A pending
    /// [`Ui::rebuild`] scroll target is applied between the two syncs, because
    /// it can only be clamped once the content height is known.
    pub fn layout(&mut self, viewport: ViewportSize) {
        draw_ui::layout(&mut self.tree, viewport);
        self.tree.update();
        let Some(scroll) = self.middle_scroll.as_mut() else {
            return;
        };
        let mut changed = scroll.sync(&mut self.tree);
        if let Some(target) = self.scroll_target.take() {
            scroll.scroll_to(target);
            changed |= scroll.sync(&mut self.tree);
        }
        if changed {
            draw_ui::layout(&mut self.tree, viewport);
            self.tree.update();
        }
    }

    /// Emit this frame's draw list into `ctx`.
    pub fn paint(&self, ctx: &mut PaintContext) {
        draw_ui::paint(&self.tree, ctx);
    }

    /// Route one input event. The app maps platform events to
    /// [`InputEvent`] first.
    pub fn route_input(&mut self, event: &InputEvent) {
        let _ = draw_ui::route_input(&mut self.tree, event);
        // Input can change hover / press / focus / the scroll offset, so the
        // next present must rebuild the draw list.
        self.repaint = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgb_systems::SystemId;
    use draw_core::{InputEvent, PointerButton, Size, Vec2};
    use draw_theme::{default_theme, Mode};

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
}
