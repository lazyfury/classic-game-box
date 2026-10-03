//! Builds the igui tree from a [`ViewModel`].
//!
//! Three columns, following the old Electron front end: a narrow icon rail on
//! the left picks what the middle column shows (the game library or settings);
//! the right column is the console, which the settings page and the screenshot
//! preview take over. A slim header and a status line top and tail the shell.
//!
//! ```text
//! Flex::column()                     <- layout root, one anchor-sized child
//!   └─ Flex::column()                <- the vertical stack (flex starts here)
//!        ├─ header
//!        ├─ Flex::row()              <- the three columns
//!        │    ├─ rail         (64px, shrink 0)   icon + label sections
//!        │    ├─ middle       (draggable, shrink 0)  library grid / settings nav
//!        │    ├─ resize handle (6px gutter)       drags the middle column
//!        │    └─ right        (grow 1)           console / settings detail
//!        └─ status bar
//! ```
//!
//! This still uses only public ``igui_components`` APIs. Callbacks push
//! [`Action`]s into an [`ViewBridge`] queue; the app drains them after routing
//! input.
//!
//! ## Layout shape (matters)
//!
//! igui's layout root places its direct children by **anchors**, and flex
//! starts one level down (see `examples/file_browser/src/ui.rs`). So the row
//! of columns must live inside the root column, not at the root itself.
//!
//! ## Module layout
//!
//! This module owns the shell and the wiring; each feature is its own module:
//!
//! - `components` — the toolbar chip, the cover icon buttons, the minimal text
//!   field, and the grid virtualization helpers the pages share;
//! - `library` — the game library page, its console filter / sort / search and
//!   its cover-card grid;
//! - `screenshots` — the screenshot grid and its cover / reveal / delete
//!   controls;
//! - `saves` and `cheats` — the running game's save slots and cheat list;
//! - `settings` — the settings page: the group nav in the middle column and the
//!   selected group's cards in the right column;
//! - `play` — the right column: the console, the immersive fullscreen view and
//!   the screenshot preview it swaps in;
//! - `tests` — the behaviour tests for all of the above.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui::igui_components::{
    Column, Component, Divider, Flex, NodeRef, Panel, ResizeHandle, Row, ScrollViewState, Text,
};
use igui::igui_core::{Color, Edges};
use igui::igui_scene::{SceneChild, SceneTree};
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, MouseFilter, SizeBasis, SurfaceStyle, TextEdit};

use crate::ui::icons::{Icon as SvgIcon, IconName};
use crate::ui::model::{Action, Section, StatusKind, ViewModel};

mod cheats;
mod components;
mod format;
mod inspector;
mod library;
pub mod menus;
mod play;
mod saves;
mod screenshots;
mod settings;

use cheats::cheats_page;
use inspector::{inspector_detail, inspector_page};
use library::{library_grid_window, library_page};
use play::play_column;
use saves::saves_page;
use screenshots::{screenshots_grid_window, screenshots_page};
use settings::{settings_detail, settings_page};

#[cfg(test)]
mod tests;

/// The rail's fixed width in logical pixels.
const RAIL_WIDTH: f32 = 64.0;

/// The middle column's width limits in logical pixels. The resize handle
/// clamps the shared width cell to this range, and the app clamps the saved
/// width to it on startup.
pub const CONTENT_MIN_WIDTH: f32 = 300.0;
pub const CONTENT_MAX_WIDTH: f32 = 640.0;

/// The middle column's width before the player drags the divider.
pub const CONTENT_DEFAULT_WIDTH: f32 = 320.0;

/// The library / screenshots grid column limits. The count follows the middle
/// column's width, so a wider pane shows more cards per row.
pub const MIN_LIBRARY_COLUMNS: usize = 2;
pub const MAX_LIBRARY_COLUMNS: usize = 4;

/// The card width the column count aims for: the widest pane packs four
/// columns near this width, and the narrowest still gets the minimum two.
const TARGET_CARD_WIDTH: f32 = 128.0;

/// The library grid's column count for a middle-column width, stepping
/// 2 / 3 / 4 as the pane widens.
pub fn library_columns(width: f32) -> usize {
    let content = (width - 2.0 * space::MD).max(0.0);
    let columns = ((content + space::SM) / (TARGET_CARD_WIDTH + space::SM)).floor() as usize;
    columns.clamp(MIN_LIBRARY_COLUMNS, MAX_LIBRARY_COLUMNS)
}

/// Height of a card's cover placeholder in logical pixels.
const PLACEHOLDER_HEIGHT: f32 = 112.0;

/// A library card's fixed height. Uniform rows let the grid mount only the
/// visible ones: the content is padded to this, and every card's text is one
/// line, so it is never taller. Sized for the cover, the name, the meta line,
/// an optional tags line and the bottom hint/play row.
const CARD_HEIGHT: f32 = 202.0;

/// A screenshot cell's fixed height, for the same virtualization.
const SHOT_HEIGHT: f32 = 180.0;

/// The card controls' icon size, and the square tap target around them.
const CARD_ICON: f32 = 12.0;
const CARD_ICON_BUTTON: f32 = 16.0;

/// Where view callbacks deposit what the user did. The app drains it once per
/// frame (see the igui UI guide's "state lives in cells" rule).
///
/// It also carries the tooltip text the current tree registered: the view
/// cannot open an overlay (the host owns the overlay layer), so it records
/// `control → tooltip` here and [`crate::ui::Ui`] opens the tip after layout.
#[derive(Clone, Default)]
pub struct ViewBridge {
    queue: Rc<RefCell<Vec<Action>>>,
    tips: Rc<RefCell<Vec<(NodeRef, String)>>>,
    /// The live state of the in-progress text edit. The app seeds it when an
    /// edit starts; the view mounts a `TextInput` from it and writes the new
    /// handle back, so a rebuild keeps the caret / selection.
    edit: Rc<RefCell<Option<Rc<RefCell<TextEdit>>>>>,
}

impl ViewBridge {
    /// Record an action.
    pub fn push(&self, action: Action) {
        self.queue.borrow_mut().push(action);
    }

    /// Take everything recorded since the last drain.
    pub fn drain(&self) -> Vec<Action> {
        std::mem::take(&mut *self.queue.borrow_mut())
    }

    /// The current in-progress text edit, if any.
    pub fn edit(&self) -> Rc<RefCell<Option<Rc<RefCell<TextEdit>>>>> {
        self.edit.clone()
    }

    /// The text of the in-progress edit (empty when there is none).
    pub fn edit_text(&self) -> String {
        self.edit
            .borrow()
            .as_ref()
            .map(|edit| edit.borrow().text().to_string())
            .unwrap_or_default()
    }

    /// Replace the in-progress edit state (creating it, or after a rebuild).
    pub fn set_edit(&self, edit: Rc<RefCell<TextEdit>>) {
        *self.edit.borrow_mut() = Some(edit);
    }

    /// Drop the in-progress edit state.
    pub fn clear_edit(&self) {
        *self.edit.borrow_mut() = None;
    }

    /// Register a tooltip for the control mounted into `node`.
    pub fn tip(&self, node: &NodeRef, text: impl Into<String>) {
        self.tips.borrow_mut().push((node.clone(), text.into()));
    }

    /// Take the tooltips registered by the tree just built.
    pub fn take_tips(&self) -> Vec<(NodeRef, String)> {
        std::mem::take(&mut *self.tips.borrow_mut())
    }
}

/// The host-owned view state a tree build reads and writes across frames,
/// bundled so [`build`] takes one handle instead of four out-params.
pub struct ViewMount {
    /// The shared width cell the resize handle writes and the middle panel
    /// reads; the caller owns it so the width survives a rebuild.
    pub content_width: Rc<Cell<f32>>,
    /// The resize handle's mounted node, re-armed after a rebuild drops the
    /// tree's pointer capture (a column-count change rebuilds mid-drag).
    pub resize_handle: NodeRef,
    /// The row range the active virtualized grid mounted in this tree, so the
    /// host can tell whether a scroll still fits inside it.
    pub mounted_rows: Cell<(usize, usize)>,
    /// The play column's live info text node (FPS / resolution), updated in
    /// place so a live readout does not rebuild the tree.
    pub info_label: NodeRef,
}

impl ViewMount {
    /// A fresh mount with the middle column clamped to the allowed width.
    pub fn new(content_width: f32) -> Self {
        Self {
            content_width: Rc::new(Cell::new(
                content_width.clamp(CONTENT_MIN_WIDTH, CONTENT_MAX_WIDTH),
            )),
            resize_handle: NodeRef::new(),
            mounted_rows: Cell::new((0, 0)),
            info_label: NodeRef::new(),
        }
    }
}

/// A built page: the middle column's content, plus the scroll state the host
/// must keep driving when the page scrolls.
pub(super) struct Page {
    pub tree: Column,
    pub scroll: Option<ScrollViewState>,
}

/// Build the whole tree for one frame, plus the persistent view state the app
/// must drive across frames (the library grid's scroll offset).
///
/// `mount` carries the host-owned view state (the shared width, the resize
/// handle, the mounted grid rows, the info label); the caller owns it so the
/// width survives a rebuild.
pub fn build(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
    mount: &ViewMount,
) -> (SceneTree, Option<ScrollViewState>) {
    mount.mounted_rows.set(grid_window(model));
    // A fullscreen transition hides the tree while the OS animates the window;
    // the caller mounts the target UI once it settles.
    if model.ui_hidden {
        mount.mounted_rows.set((0, 0));
        return (
            Flex::column().mouse_filter(MouseFilter::Ignore).into_tree(),
            None,
        );
    }
    // Immersive play: fullscreen for now reuses the right-column play view
    // (title, live info, picture, controls, core), just without the shell.
    if model.fullscreen {
        mount.mounted_rows.set((0, 0));
        let tree = Flex::column()
            .mouse_filter(MouseFilter::Ignore)
            .child(play_column(theme, model, actions, &mount.info_label))
            .into_tree();
        return (tree, None);
    }
    let mut scroll = None;
    // The resize handle points at the middle panel, so bind a slot before the
    // panel is built and read it into the handle.
    let content_ref = NodeRef::new();
    // The layout root places its direct children by anchors, so the vertical
    // stack is one level down: the root's single child is a column, and *its*
    // children (header / columns / status) are the flex items.
    let inner = Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(header(theme, model))
        .child(
            Flex::row()
                .grow(1.0)
                .gap(0.0)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(rail(theme, model, actions))
                .child(
                    content_column(theme, model, actions, &mut scroll, &mount.content_width)
                        .ref_(&content_ref),
                )
                .child(
                    resize_handle(theme, &mount.content_width, content_ref)
                        .ref_(&mount.resize_handle),
                )
                .child(right_column(
                    theme,
                    model,
                    actions,
                    &mount.info_label,
                    &mut scroll,
                )),
        );
    let tree = Flex::column()
        .mouse_filter(MouseFilter::Ignore)
        .child(inner.child(status_bar(theme, model)))
        .into_tree();
    (tree, scroll)
}

/// The right column: normally the console, but the settings page takes it over
/// (like the screenshot preview) so its groups get a wide canvas. A preview
/// still wins, so it behaves the same as on every other page.
fn right_column(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
    info_ref: &NodeRef,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    if model.section == Section::Settings && model.preview.is_none() {
        let page = settings_detail(theme, model, actions);
        *scroll = page.scroll;
        return page.tree;
    }
    if model.section == Section::Inspector && model.preview.is_none() {
        return inspector_detail(theme, model, actions);
    }
    play_column(theme, model, actions, info_ref)
}

/// The slim top bar: the app name and what the middle column is showing.
///
/// It leaves the platform's safe area at the top and left, so on macOS the
/// content running under the title bar does not hide the name behind the
/// traffic lights.
// The call is commented out in `build` while the native title bar is tried;
// keep the builder around until that settles.
fn header(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    let safe = model.safe_area;
    Column::new()
        .gap(0.0)
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::new(
                    space::LG + safe.left,
                    space::SM + safe.top,
                    space::LG,
                    space::SM,
                ))
                .min_size(0.0, 40.0 + safe.top)
                .child(Text::subheading("Classic Game Box", theme).bold())
                .child(Text::caption(model.section.label(), theme).tone(Tone::Muted)),
        )
        .child(Divider::horizontal(theme))
}

/// The left rail: one icon-over-label button per section (the VS Code
/// activity bar, with the names always visible).
fn rail(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Column {
    let mut rail = Column::new()
        .gap(space::SM)
        .padding(Edges::new(space::XS, space::SM, space::XS, space::SM))
        .basis(SizeBasis::Px(RAIL_WIDTH))
        .shrink(0.0)
        .surface(SurfaceStyle::new(theme.palette().surface))
        .mouse_filter(MouseFilter::Ignore);
    for section in Section::ALL {
        rail = rail.child(rail_item(theme, section, model.section, actions));
    }
    rail
}

/// One rail entry. Selected is the accent fill; hover is the only other state.
fn rail_item(
    theme: &'static dyn Theme,
    section: Section,
    active: Section,
    actions: &ViewBridge,
) -> Column {
    let actions = actions.clone();
    let selected = section == active;
    let ink = if selected {
        theme.palette().on_accent
    } else {
        theme.palette().muted
    };
    let item = Column::new()
        .gap(space::XXS)
        .padding(Edges::new(space::XXS, space::SM, space::XXS, space::SM))
        .align(Align::Center)
        .dynamic_background(move |state| {
            let fill = if selected {
                theme.palette().accent
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::MD)
        })
        .on_click(move |_tree, _id| actions.push(Action::Show(section)));
    // Every section uses a vendored SVG icon, so the rail is one stroke set.
    let icon = match section {
        Section::Library => IconName::Library,
        Section::Screenshots => IconName::Camera,
        Section::Inspector => IconName::LayoutGrid,
        Section::Saves => IconName::Save,
        Section::Cheats => IconName::Sparkles,
        Section::Settings => IconName::Settings2,
    };
    item.child(SvgIcon::new(icon, ink, 20.0)).child(
        Text::caption(section.label(), theme)
            .color(ink)
            .max_lines(1),
    )
}

/// The content column: a resizable panel holding the current page. Its width
/// comes from the shared cell the resize handle drives.
fn content_column(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
    scroll: &mut Option<ScrollViewState>,
    content_width: &Rc<Cell<f32>>,
) -> Panel {
    let page = match model.section {
        Section::Library => library_page(theme, model, actions),
        Section::Screenshots => screenshots_page(theme, model, actions),
        Section::Inspector => inspector_page(theme, model, actions),
        Section::Saves => saves_page(theme, model, actions),
        Section::Cheats => cheats_page(theme, model, actions),
        Section::Settings => settings_page(theme, model, actions),
    };
    *scroll = page.scroll;
    Panel::new()
        .color(theme.palette().surface_raised)
        .flat()
        .basis(SizeBasis::Px(content_width.get()))
        .shrink(0.0)
        .clip(true)
        .mouse_filter(MouseFilter::Ignore)
        .child(page.tree.grow(1.0))
}

/// The draggable divider between the middle column and the console.
///
/// [`ResizeHandle`] updates the target panel's flex basis through the shared
/// width cell, so a drag re-lays-out without rebuilding the tree; the app
/// reads the cell back to persist the width.
fn resize_handle(
    theme: &'static dyn Theme,
    content_width: &Rc<Cell<f32>>,
    target: NodeRef,
) -> ResizeHandle {
    ResizeHandle::vertical(theme)
        .target(target)
        .width(content_width.clone())
        .min(CONTENT_MIN_WIDTH)
        .max(CONTENT_MAX_WIDTH)
        .color(theme.palette().border)
}

/// The inclusive row range the active virtualized grid mounts for the model's
/// current scroll offset and viewport.
///
/// The app compares this (via [`Ui::grid_window_covers`](crate::ui::Ui::grid_window_covers))
/// to the range already in the tree, so scrolling inside the mounted window is a
/// plain repaint instead of a rebuild.
pub fn grid_window(model: &ViewModel) -> (usize, usize) {
    match model.section {
        Section::Library => library_grid_window(model),
        Section::Screenshots => screenshots_grid_window(model),
        _ => (0, 0),
    }
}

/// The bottom status line: the last app message, plus the save hotkeys.
fn status_bar(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    let (status, tone) = if model.status.is_empty() {
        ("就绪", Tone::Muted)
    } else {
        let tone = match model.status_kind {
            StatusKind::Info => Tone::Muted,
            StatusKind::Success => Tone::Success,
            StatusKind::Error => Tone::Error,
        };
        (model.status.as_str(), tone)
    };
    Column::new()
        .gap(0.0)
        .child(Divider::horizontal(theme))
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::new(space::MD, space::XXS, space::MD, space::XXS))
                .min_size(0.0, 22.0)
                .child(
                    Text::caption(status, theme)
                        .tone(tone)
                        .grow(1.0)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(
                    Text::caption(
                        "F5 存档 / F6 读档 / F12 截图 / ⇧F12 封面 / 退格 倒带",
                        theme,
                    )
                    .tone(Tone::Subtle),
                ),
        )
}
