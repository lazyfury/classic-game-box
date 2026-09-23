//! Builds the quill tree from a [`ViewModel`].
//!
//! Three columns, following the old Electron front end: a narrow icon rail on
//! the left picks what the middle column shows (the game library or settings);
//! the right column is the console and is always mounted. A slim header and a
//! status line top and tail the shell.
//!
//! ```text
//! Flex::column()                     <- layout root, one anchor-sized child
//!   └─ Flex::column()                <- the vertical stack (flex starts here)
//!        ├─ header
//!        ├─ Flex::row()              <- the three columns
//!        │    ├─ rail         (64px, shrink 0)   icon + label sections
//!        │    ├─ middle       (draggable, shrink 0)  library grid / settings
//!        │    ├─ resize handle (6px gutter)       drags the middle column
//!        │    └─ play column  (grow 1)           the console, always
//!        └─ status bar
//! ```
//!
//! This still uses only public `draw_components` APIs. Callbacks push
//! [`Action`]s into an [`Actions`] queue; the app drains them after routing
//! input.
//!
//! ## Layout shape (matters)
//!
//! quill's layout root places its direct children by **anchors**, and flex
//! starts one level down (see `examples/file_browser/src/ui.rs`). So the row
//! of columns must live inside the root column, not at the root itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use draw_components::{
    Badge, Button, Card, Column, Component, Divider, EmptyState, Flex, Grid, NodeRef, Panel,
    ResizeHandle, Row, ScrollView, ScrollViewState, Text,
};
use draw_core::{Color, Edges};
use draw_render::Paint;
use draw_scene::{SceneChild, SceneTree};
use draw_theme::radius::MD;
use draw_theme::{radius, space, Theme, Tone};
use draw_ui::{Align, Justify, MouseFilter, SizeBasis, SurfaceStyle, Track};

use crate::frame::{cover_fit, FrameImage};
use crate::icons::{Icon as SvgIcon, IconName};
use crate::model::{
    Action, CheatRow, Confirm, EditKind, EditState, GameRow, SaveSlotRow, ScreenshotRow, Section,
    ShaderKind, SortKey, ViewModel,
};

/// The rail's fixed width in logical pixels.
const RAIL_WIDTH: f32 = 64.0;

/// The middle column's width limits in logical pixels. The resize handle
/// clamps the shared width cell to this range, and the app clamps the saved
/// width to it on startup.
pub const MIDDLE_MIN_WIDTH: f32 = 300.0;
pub const MIDDLE_MAX_WIDTH: f32 = 640.0;

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
/// line, so it is never taller.
const CARD_HEIGHT: f32 = 184.0;

/// A screenshot cell's fixed height, for the same virtualization.
const SHOT_HEIGHT: f32 = 180.0;

/// The card controls' icon size, and the square tap target around them.
const CARD_ICON: f32 = 12.0;
const CARD_ICON_BUTTON: f32 = 16.0;

/// The pinned pin's colour (the old front end's `#ffd60a`).
const PIN_COLOR: Color = Color::new(1.0, 0.84, 0.04, 1.0);

/// Where view callbacks deposit what the user did. The app drains it once per
/// frame (see the quill UI guide's "state lives in cells" rule).
#[derive(Clone, Default)]
pub struct Actions {
    queue: Rc<RefCell<Vec<Action>>>,
}

impl Actions {
    /// Record an action.
    pub fn push(&self, action: Action) {
        self.queue.borrow_mut().push(action);
    }

    /// Take everything recorded since the last drain.
    pub fn drain(&self) -> Vec<Action> {
        std::mem::take(&mut *self.queue.borrow_mut())
    }
}

/// Build the whole tree for one frame, plus the persistent view state the app
/// must drive across frames (the library grid's scroll offset).
///
/// `middle_width` is the shared width cell the resize handle writes and the
/// middle panel reads; the caller owns it so the width survives a rebuild.
/// `handle_ref` receives the resize handle's node, so the caller can re-arm a
/// drag across a rebuild (a column-count change rebuilds mid-drag).
/// `mounted_rows` is set to the row range the active virtualized grid mounts,
/// so the caller can tell whether a scroll still fits inside it.
pub fn build(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    middle_width: &Rc<Cell<f32>>,
    handle_ref: &NodeRef,
    mounted_rows: &Cell<(usize, usize)>,
) -> (SceneTree, Option<ScrollViewState>) {
    mounted_rows.set(grid_window(model));
    let mut middle_scroll = None;
    // The resize handle points at the middle panel, so bind a slot before the
    // panel is built and read it into the handle.
    let middle_ref = NodeRef::new();
    // The layout root places its direct children by anchors, so the vertical
    // stack is one level down: the root's single child is a column, and *its*
    // children (header / columns / status) are the flex items.
    let mut inner = Flex::column()
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
                    middle(theme, model, actions, &mut middle_scroll, middle_width)
                        .ref_(&middle_ref),
                )
                .child(resize_handle(theme, middle_width, middle_ref).ref_(handle_ref))
                .child(play_column(theme, model, actions)),
        );
    if model.confirm.is_some() {
        inner = inner.child(confirm_bar(theme, model, actions));
    }
    let tree = Flex::column()
        .mouse_filter(MouseFilter::Ignore)
        .child(inner.child(status_bar(theme, model)))
        .into_tree();
    (tree, middle_scroll)
}

/// The confirmation bar, above the status line, for a pending destructive
/// action. Its buttons run the action or dismiss it.
fn confirm_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let message = model.confirm.map(Confirm::message).unwrap_or_default();
    let confirm = actions.clone();
    let cancel = actions.clone();
    Column::new()
        .gap(0.0)
        .child(Divider::horizontal(theme))
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::new(space::MD, space::XS, space::MD, space::XS))
                .child(
                    Text::small(message, theme)
                        .grow(1.0)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(
                    Button::destructive("删除", theme)
                        .on_click(move || confirm.push(Action::ConfirmDelete)),
                )
                .child(
                    Button::secondary("取消", theme)
                        .on_click(move || cancel.push(Action::CancelDelete)),
                ),
        )
}

/// The slim top bar: the app name and what the middle column is showing.
///
/// It leaves the platform's safe area at the top and left, so on macOS the
/// content running under the title bar does not hide the name behind the
/// traffic lights.
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
fn rail(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
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
    actions: &Actions,
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
        .on_click(move || actions.push(Action::Show(section)));
    // Every section uses a vendored SVG icon, so the rail is one stroke set.
    let icon = match section {
        Section::Library => IconName::Library,
        Section::Screenshots => IconName::Camera,
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

/// The middle column: a resizable panel holding the current page. Its width
/// comes from the shared cell the resize handle drives.
fn middle(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    middle_scroll: &mut Option<ScrollViewState>,
    middle_width: &Rc<Cell<f32>>,
) -> Panel {
    let page = match model.section {
        Section::Library => library_page(theme, model, actions, middle_scroll),
        Section::Screenshots => screenshots_page(theme, model, actions, middle_scroll),
        Section::Saves => saves_page(theme, model, actions, middle_scroll),
        Section::Cheats => cheats_page(theme, model, actions, middle_scroll),
        Section::Settings => settings_page(theme, model, actions, middle_scroll),
    };
    Panel::new()
        .color(theme.palette().surface_raised)
        .flat()
        .basis(SizeBasis::Px(middle_width.get()))
        .shrink(0.0)
        .clip(true)
        .mouse_filter(MouseFilter::Ignore)
        .child(page.grow(1.0))
}

/// The draggable divider between the middle column and the console.
///
/// [`ResizeHandle`] updates the target panel's flex basis through the shared
/// width cell, so a drag re-lays-out without rebuilding the tree; the app
/// reads the cell back to persist the width.
fn resize_handle(
    theme: &'static dyn Theme,
    middle_width: &Rc<Cell<f32>>,
    target: NodeRef,
) -> ResizeHandle {
    ResizeHandle::vertical(theme)
        .target(target)
        .width(middle_width.clone())
        .min(MIDDLE_MIN_WIDTH)
        .max(MIDDLE_MAX_WIDTH)
        .color(theme.palette().border)
}

/// The console column, on the right and always mounted. While a screenshot is
/// being previewed it shows the picture instead of the console.
fn play_column(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    if let Some(id) = model.preview {
        if let Some(shot) = model.screenshots.iter().find(|shot| shot.id == id) {
            return preview_column(theme, model, shot, actions);
        }
    }

    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(space::MD))
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore);

    // A game launched from `--rom` may not be in the library list, so the
    // title falls back to the core name rather than the selected row.
    let title = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.name.clone())
        .or_else(|| (!model.core_name.is_empty()).then(|| model.core_name.clone()))
        .unwrap_or_else(|| "没有选中游戏".to_string());
    column = column.child(Text::subheading(title, theme).max_lines(1).ellipsis(true));

    // The framebuffer itself: it grows into the remaining space and centres a
    // letterboxed picture inside whatever rectangle it gets.
    match &model.frame {
        Some(frame) => {
            column =
                column.child(FrameImage::new(frame.texture, frame.width, frame.height).grow(1.0));
        }
        None => {
            column = column.child(
                Flex::row()
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .grow(1.0)
                    .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::MD))
                    .child(Text::small("没有画面：还没有载入游戏。", theme).tone(Tone::Muted)),
            );
        }
    }

    let pause = if model.paused { "继续" } else { "暂停" };
    let mut controls = Row::new().gap(space::SM);
    let toggle = actions.clone();
    controls = controls
        .child(Button::primary(pause, theme).on_click(move || toggle.push(Action::TogglePause)));
    let reset = actions.clone();
    controls = controls
        .child(Button::secondary("复位", theme).on_click(move || reset.push(Action::Reset)));
    let rewind = actions.clone();
    controls =
        controls.child(Button::ghost("倒带", theme).on_click(move || rewind.push(Action::Rewind)));
    let save = actions.clone();
    controls = controls.child(
        Button::secondary("快速存档", theme).on_click(move || save.push(Action::SaveState(0))),
    );
    let load = actions.clone();
    controls = controls.child(
        Button::secondary("快速读档", theme).on_click(move || load.push(Action::LoadState(0))),
    );
    let shot = actions.clone();
    controls = controls
        .child(Button::ghost("截图", theme).on_click(move || shot.push(Action::Screenshot)));
    let cover = actions.clone();
    controls = controls.child(
        Button::ghost("设为封面", theme).on_click(move || cover.push(Action::ScreenshotCover)),
    );
    column = column.child(controls);

    if !model.core_name.is_empty() {
        column = column
            .child(Text::caption(format!("核心：{}", model.core_name), theme).tone(Tone::Muted));
    }
    column
}

fn library_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    library_scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(Text::heading("游戏库", theme));
    column = column.child(sort_bar(theme, model, actions));
    column = column.child(search_bar(theme, model, actions));
    if let Some(edit) = &model.editing {
        if edit.kind != EditKind::Search {
            column = column.child(edit_bar(theme, edit, actions));
        }
    }

    if model.games.is_empty() {
        let empty = if model.search.is_empty() {
            EmptyState::new("还没有游戏", theme).description("把 ROM 拖进来，或点“添加游戏文件…”。")
        } else {
            EmptyState::new("没有匹配的游戏", theme).description("换个关键词试试。")
        };
        column = column.child(empty);
    } else {
        // Only the grid scrolls; the title and the add buttons stay put.
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(library_grid(theme, model, actions));
        *library_scroll = Some(view.state());
        column = column.child(view);
    }

    let add_files = actions.clone();
    let add_dir = actions.clone();
    column = column.child(
        Row::new()
            .gap(space::SM)
            .child(
                Button::secondary("添加游戏文件…", theme)
                    .on_click(move || add_files.push(Action::AddGames)),
            )
            .child(
                Button::ghost("添加游戏目录…", theme)
                    .on_click(move || add_dir.push(Action::OpenRom)),
            ),
    );
    column
}

/// The library's sort controls: one chip per key, then a direction toggle.
/// Pinned games are always on top, so the key only orders within the groups.
fn sort_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Row {
    let mut bar = Row::new()
        .align(Align::Center)
        .gap(space::XS)
        .child(Text::caption("排序", theme).tone(Tone::Muted));
    for key in SortKey::ALL {
        let actions = actions.clone();
        let button = if model.sort == key {
            Button::primary(key.label(), theme)
        } else {
            Button::ghost(key.label(), theme)
        };
        bar = bar.child(
            button
                .mini()
                .on_click(move || actions.push(Action::Sort(key))),
        );
    }
    let toggle = actions.clone();
    let arrow = if model.sort_desc {
        IconName::ArrowDown
    } else {
        IconName::ArrowUp
    };
    bar.child(
        Button::ghost("", theme)
            .mini()
            .child(SvgIcon::new(arrow, theme.palette().foreground, CARD_ICON))
            .on_click(move || toggle.push(Action::ToggleSortOrder)),
    )
}

/// The search row: a button that opens the field, or the field while typing,
/// with a clear control when a query is set.
fn search_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Row {
    let mut row = Row::new().align(Align::Center).gap(space::XS);
    if let Some(edit) = &model.editing {
        if edit.kind == EditKind::Search {
            let done = actions.clone();
            let clear = actions.clone();
            return row
                .child(edit_field(theme, &edit.text, edit.caret).grow(1.0))
                .child(
                    Button::primary("完成", theme)
                        .mini()
                        .on_click(move || done.push(Action::CommitEdit)),
                )
                .child(
                    Button::ghost("清除", theme)
                        .mini()
                        .on_click(move || clear.push(Action::ClearSearch)),
                );
        }
    }
    let start = actions.clone();
    let label = if model.search.is_empty() {
        "搜索…".to_string()
    } else {
        format!("搜索：{}", model.search)
    };
    row = row.child(
        Button::ghost(label, theme)
            .mini()
            .on_click(move || start.push(Action::StartSearch)),
    );
    if !model.search.is_empty() {
        let clear = actions.clone();
        row = row.child(icon_button(
            IconName::Close,
            theme.palette().foreground,
            move || clear.push(Action::ClearSearch),
        ));
    }
    row
}

/// The rename / tags edit bar: a text field, the caret, and save / cancel.
fn edit_bar(theme: &'static dyn Theme, edit: &EditState, actions: &Actions) -> Column {
    let label = match edit.kind {
        EditKind::Name => "改名",
        EditKind::Tags => "标签（用逗号分隔）",
        EditKind::Search => "搜索",
    };
    let save = actions.clone();
    let cancel = actions.clone();
    Column::new()
        .gap(space::XS)
        .child(Text::caption(label, theme).tone(Tone::Muted))
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::XS)
                .child(edit_field(theme, &edit.text, edit.caret).grow(1.0))
                .child(
                    Button::primary("保存", theme)
                        .mini()
                        .on_click(move || save.push(Action::CommitEdit)),
                )
                .child(
                    Button::ghost("取消", theme)
                        .mini()
                        .on_click(move || cancel.push(Action::CancelEdit)),
                ),
        )
}

/// A minimal text field: the text with a caret bar drawn between the two
/// halves. quill has no `TextInput`, so the app owns the keyboard and this only
/// renders the current state.
fn edit_field(theme: &'static dyn Theme, text: &str, caret: usize) -> Flex {
    let caret = caret.min(text.len());
    let (before, after) = text.split_at(caret);
    Flex::row()
        .align(Align::Center)
        .gap(0.0)
        .padding(Edges::new(space::SM, space::XS, space::SM, space::XS))
        .min_size(0.0, 26.0)
        .surface(
            SurfaceStyle::new(theme.palette().surface_raised)
                .border(theme.palette().accent)
                .radius(radius::SM),
        )
        .child(Text::small(before, theme).max_lines(1))
        .child(Text::small("|", theme).color(theme.palette().accent))
        .child(Text::small(after, theme).max_lines(1).ellipsis(true))
}

/// The library as a fixed-column grid of cover cards, mounted a window at a
/// time. The whole thing is the content of the library page's [`ScrollView`].
///
/// Only the rows the viewport covers are built (plus one row of slack), with
/// spacers above and below standing in for the rest, so the scrollbar and the
/// offset stay correct while the cost of a scroll step depends on the viewport
/// rather than on how many games the library holds.
fn library_grid(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let total = model.games.len();
    let mut column = Column::new().gap(0.0).padding(Edges::ZERO);
    if total == 0 {
        return column;
    }
    let columns = model.grid_columns.max(1);
    let rows = total.div_ceil(columns);
    let stride = CARD_HEIGHT + space::SM;
    let (first, last) = library_grid_window(model);

    let top = first as f32 * stride;
    if top > 0.0 {
        column = column.child(spacer(top));
    }

    let mut grid = Grid::new(vec![Track::Fr(1.0); columns])
        .gap(space::SM)
        .padding(Edges::ZERO);
    for row in first..=last {
        for column_index in 0..columns {
            let index = row * columns + column_index;
            if index >= total {
                break;
            }
            grid = grid.child(game_card(theme, model, &model.games[index], index, actions));
        }
    }
    column = column.child(grid);

    let bottom = rows.saturating_sub(last + 1) as f32 * stride;
    if bottom > 0.0 {
        column = column.child(spacer(bottom));
    }
    column
}

/// An empty block of `height` logical pixels, standing in for unmounted rows.
fn spacer(height: f32) -> Flex {
    Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .min_size(0.0, height)
}

/// The inclusive row range to mount for a scroll `offset` and `viewport`,
/// given the row count and stride. One row of slack past each edge keeps a
/// partly visible row from popping in.
fn visible_rows(offset: f32, viewport: f32, rows: usize, stride: f32) -> (usize, usize) {
    if rows == 0 || stride <= 0.0 {
        return (0, 0);
    }
    let last = rows - 1;
    let first = (offset.max(0.0) / stride).floor() as usize;
    let end = ((offset + viewport) / stride).ceil() as usize + 1;
    (first.min(last), end.min(last).max(first.min(last)))
}

/// The rows a grid of `total` items mounts: `total.div_ceil(columns)` rows,
/// windowed around the viewport by [`visible_rows`].
fn window_for(
    total: usize,
    columns: usize,
    offset: f32,
    viewport: f32,
    stride: f32,
) -> (usize, usize) {
    let rows = total.div_ceil(columns.max(1));
    visible_rows(offset, viewport, rows, stride)
}

/// Before the first layout the viewport is unknown; assume a screenful.
fn grid_viewport(model: &ViewModel) -> f32 {
    if model.grid_viewport > 0.0 {
        model.grid_viewport
    } else {
        640.0
    }
}

/// The library grid's mounted row range for the model's scroll state.
fn library_grid_window(model: &ViewModel) -> (usize, usize) {
    window_for(
        model.games.len(),
        model.grid_columns,
        model.grid_offset,
        grid_viewport(model),
        CARD_HEIGHT + space::SM,
    )
}

/// The screenshots grid's mounted row range for the model's scroll state. It
/// follows the same filter as [`screenshots_page`].
fn screenshots_grid_window(model: &ViewModel) -> (usize, usize) {
    let total = model
        .screenshots
        .iter()
        .filter(|shot| Some(shot.game_id) == model.screenshot_game)
        .count();
    window_for(
        total,
        model.grid_columns,
        model.grid_offset,
        grid_viewport(model),
        SHOT_HEIGHT + space::SM,
    )
}

/// The inclusive row range the active virtualized grid mounts for the model's
/// current scroll offset and viewport.
///
/// The app compares this (via [`Ui::grid_window_covers`](crate::Ui::grid_window_covers))
/// to the range already in the tree, so scrolling inside the mounted window is a
/// plain repaint instead of a rebuild.
pub fn grid_window(model: &ViewModel) -> (usize, usize) {
    match model.section {
        Section::Library => library_grid_window(model),
        Section::Screenshots => screenshots_grid_window(model),
        _ => (0, 0),
    }
}

/// One library cell: the cover (with the console badge and the card controls
/// over it), then the name under it. Clicking the cover or name starts the
/// game; the controls are their own buttons, so they never start it (the
/// nearest callback wins).
fn game_card(
    theme: &'static dyn Theme,
    model: &ViewModel,
    game: &GameRow,
    index: usize,
    actions: &Actions,
) -> Column {
    let click = actions.clone();
    let playing = model.selected == Some(index);
    Column::new()
        .gap(space::XXS)
        .padding(Edges::all(space::XXS))
        // A fixed height keeps the rows uniform for the virtualized grid; the
        // tags line is always present so a card with no tags is not shorter.
        .min_size(0.0, CARD_HEIGHT)
        .dynamic_background(move |state| {
            let fill = if playing {
                theme.palette().selection
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::MD)
        })
        .on_click(move || click.push(Action::Play(index)))
        .child(cover(theme, game, index, actions))
        .child(
            Text::small(game.name.as_str(), theme)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(
            Text::caption(meta_label(game), theme)
                .tone(Tone::Subtle)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(
            Text::caption(tags_label(game), theme)
                .tone(Tone::Muted)
                .max_lines(1)
                .ellipsis(true),
        )
}

/// A card's tags as one line: the first few `#words`, then a `+N` count. A
/// single ellipsized line, because the cells are narrow and a wrapping row of
/// chips would make every card in the row taller.
fn tags_label(game: &GameRow) -> String {
    const SHOWN: usize = 3;
    let shown: Vec<String> = game
        .tags
        .iter()
        .take(SHOWN)
        .map(|tag| format!("#{tag}"))
        .collect();
    let label = shown.join(" ");
    let hidden = game.tags.len().saturating_sub(SHOWN);
    if hidden > 0 {
        format!("{label} +{hidden}")
    } else {
        label
    }
}

/// A card's cover: the console badge top-left and the pin / delete controls
/// top-right, over either the game's cover screenshot or a deterministic colour
/// drawn from its ROM path (so a game without artwork keeps the same colour
/// between runs). Without a screenshot the title is clipped in and centred.
fn cover(theme: &'static dyn Theme, game: &GameRow, index: usize, actions: &Actions) -> Column {
    let mut cover = Column::new()
        .gap(space::XXS)
        // No outer padding: the controls sit flush in the top-right corner.
        .padding(Edges::ZERO)
        .min_size(0.0, PLACEHOLDER_HEIGHT)
        .surface(SurfaceStyle::new(cover_color(&game.path)).radius(radius::SM))
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::XXS)
                .padding(Edges {
                    left: 2.0,
                    top: 2.0,
                    right: 2.0,
                    bottom: 2.0,
                })
                .background(Color::BLACK.with_alpha(0.4))
                // The badge and the controls both hug their corner, no inset.
                .child(system_badge(theme, game))
                .child(Flex::column().grow(1.0).padding(Edges::all(0.0)))
                .child(card_controls(theme, game, index, actions)),
        );

    match game.cover {
        // A real cover fills the whole cell behind the controls. The foreground
        // paints before the children, so the badge and buttons stay on top; the
        // clip crops the overflow to the cell.
        Some(handle) => {
            cover = cover.clip(true).foreground(move |ctx, rect, _state| {
                let destination = cover_fit((handle.width, handle.height), rect);
                ctx.draw_image(handle.texture, destination, None, Paint::default());
            });
        }
        // No artwork: the name stands in for it, centred on the colour.
        None => {
            cover = cover.child(
                Flex::row()
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .grow(1.0)
                    .padding(Edges::all(space::XS))
                    .child(
                        Text::caption(game.name.as_str(), theme)
                            .color(Color::WHITE.with_alpha(0.92))
                            .max_lines(3)
                            .ellipsis(true),
                    ),
            );
        }
    }
    cover
}

/// The console badge, top-left on the cover: the short name the console is
/// known by ("NES", "GBA", "GB"). Compact and unpadded, like the controls.
fn system_badge(theme: &'static dyn Theme, game: &GameRow) -> Flex {
    Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .gap(0.0)
        .padding(Edges {
            left: 4.0,
            top: 1.0,
            right: 4.0,
            bottom: 1.0,
        })
        .min_size(CARD_ICON_BUTTON, CARD_ICON_BUTTON)
        .surface(SurfaceStyle::new(Color::new(0.11, 0.11, 0.13, 0.72)).radius(radius::SM))
        .child(Text::caption(game.system.short(), theme).color(Color::WHITE.with_alpha(0.88)))
}

/// The card's controls, top-right on the cover: the screenshot count (when
/// there are any), the pin, then delete. The pin is yellow when the game is
/// pinned; delete destroys the ROM.
fn card_controls(
    theme: &'static dyn Theme,
    game: &GameRow,
    index: usize,
    actions: &Actions,
) -> Row {
    let mut row = Row::new().align(Align::Center).gap(space::XXS);
    if game.screenshots > 0 {
        row = row.child(screenshot_entry(theme, game, actions));
    }
    let game_id = game.id;
    let ink = Color::WHITE.with_alpha(0.85);
    row = row.child(icon_button(IconName::Pencil, ink, {
        let actions = actions.clone();
        move || actions.push(Action::StartRename(game_id))
    }));
    row = row.child(icon_button(IconName::Tag, ink, {
        let actions = actions.clone();
        move || actions.push(Action::StartTagEdit(game_id))
    }));
    row.child(icon_button(
        IconName::Pin,
        if game.pinned { PIN_COLOR } else { ink },
        {
            let actions = actions.clone();
            move || actions.push(Action::TogglePin(index))
        },
    ))
    .child(icon_button(IconName::Trash, ink, {
        let actions = actions.clone();
        move || actions.push(Action::RequestDelete(Confirm::DeleteGame(game_id)))
    }))
}

/// The card's screenshot count: a camera and the number, opening the
/// screenshots section for this game. Only shown when the game has any.
fn screenshot_entry(theme: &'static dyn Theme, game: &GameRow, actions: &Actions) -> Flex {
    let actions = actions.clone();
    let game_id = game.id;
    let ink = Color::WHITE.with_alpha(0.85);
    compact_button(move || actions.push(Action::ShowScreenshots(game_id)))
        .gap(2.0)
        .child(SvgIcon::new(IconName::Camera, ink, CARD_ICON))
        .child(Text::caption(game.screenshots.to_string(), theme).color(ink))
}

/// A small, transparent-until-hovered icon button on a coloured cover. No
/// padding, so it hugs the corner; the icon is `Ignore` for input, so the
/// click lands on the button.
fn icon_button(icon: IconName, color: Color, on_click: impl FnMut() + 'static) -> Flex {
    compact_button(on_click).child(SvgIcon::new(icon, color, CARD_ICON))
}

/// The bare frame a compact icon button shares: a fixed square, a hover fill
/// and a click, with no padding of its own.
fn compact_button(on_click: impl FnMut() + 'static) -> Flex {
    Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .gap(0.0)
        .shrink(0.0)
        .padding(Edges {
            left: 2.0,
            top: 1.0,
            right: 2.0,
            bottom: 1.0,
        })
        .min_size(CARD_ICON_BUTTON, CARD_ICON_BUTTON)
        .dynamic_background(move |state| {
            let fill = if state.hovered || state.pressed {
                Color::WHITE.with_alpha(0.16)
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::SM)
        })
        .on_click(on_click)
}

/// A card's meta line: its size, and either the play count and time or the
/// fact that it has never been run.
fn meta_label(game: &GameRow) -> String {
    let mut parts = vec![format_size(game.size)];
    if game.play_count > 0 {
        parts.push(format!("玩过 {} 次", game.play_count));
        parts.push(format_duration(game.play_seconds));
    } else {
        parts.push("未玩过".to_string());
    }
    parts.join(" · ")
}

/// Bytes into the short string a card has room for.
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < MIB {
        format!("{} KB", (bytes as f64 / KIB as f64).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    }
}

/// A total play time in the units an interface has room for.
fn format_duration(seconds: i64) -> String {
    let whole = seconds.max(0);
    if whole < 60 {
        return format!("{whole} 秒");
    }
    let minutes = whole / 60;
    if minutes < 60 {
        return format!("{minutes} 分");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if hours >= 100 || rest == 0 {
        format!("{hours} 时")
    } else {
        format!("{hours} 时 {rest} 分")
    }
}

/// A stable, readable cover colour for a ROM path: hash it to a hue with a
/// fixed saturation and value, so the whole grid stays legible against light
/// text. FNV-1a, because it is short and stable across runs.
fn cover_color(path: &str) -> Color {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hsv((hash % 360) as f32, 0.45, 0.55)
}

/// HSV (h in degrees) to an RGB [`Color`]. Only used for cover hues.
fn hsv(hue: f32, saturation: f32, value: f32) -> Color {
    let chroma = value * saturation;
    let h = hue / 60.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    Color::rgb(r + m, g + m, b + m)
}

/// The screenshots section: the current game's screenshots, newest first.
/// Which game is shown comes from the model (the playing game, or one a card
/// sent us to).
/// The cheats section: the running game's cheat list, with toggles and a
/// `.cht` import.
fn cheats_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let game = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.name.clone());
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::heading("金手指", theme));
    let subtitle = match &game {
        Some(name) => format!("{name} · {}", model.core_name),
        None => "没有正在运行的游戏".to_string(),
    };
    column = column.child(Text::caption(subtitle, theme).tone(Tone::Muted));

    if !model.playing {
        column = column.child(EmptyState::new("没有正在运行的游戏", theme));
    } else if model.cheats.is_empty() {
        column = column.child(
            EmptyState::new("还没有金手指", theme).description("导入一个 RetroArch 的 .cht 文件。"),
        );
    } else {
        let mut list = Column::new().gap(space::XS);
        for (index, cheat) in model.cheats.iter().enumerate() {
            list = list.child(cheat_row(theme, index, cheat, actions));
        }
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(list);
        *scroll = Some(view.state());
        column = column.child(view);
    }

    let import = actions.clone();
    column = column.child(
        Button::secondary("导入 .cht…", theme).on_click(move || import.push(Action::ImportCheats)),
    );
    column = column.child(
        Text::caption(
            "码的语法由核心决定：Mesen 认 Game Genie / PAR / AAAA:VV，mGBA 认 GBA 码；FBNeo 不支持金手指。",
            theme,
        )
        .tone(Tone::Subtle)
        .max_lines(2),
    );
    column
}

/// One cheat: its description and code, and an on/off toggle.
fn cheat_row(theme: &'static dyn Theme, index: usize, cheat: &CheatRow, actions: &Actions) -> Row {
    let toggle = actions.clone();
    let button = if cheat.enabled {
        Button::primary("开", theme)
    } else {
        Button::ghost("关", theme)
    };
    Row::new()
        .align(Align::Center)
        .gap(space::SM)
        .child(
            Column::new()
                .gap(0.0)
                .grow(1.0)
                .child(
                    Text::small(cheat.desc.as_str(), theme)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(
                    Text::caption(cheat.code.as_str(), theme)
                        .tone(Tone::Subtle)
                        .max_lines(1)
                        .ellipsis(true),
                ),
        )
        .child(
            button
                .mini()
                .on_click(move || toggle.push(Action::ToggleCheat(index))),
        )
}

/// The saves section: the running game's save-state slots for its core.
fn saves_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let game = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.name.clone());
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::heading("存档", theme));

    let subtitle = match &game {
        Some(name) => format!("{name} · {}", model.core_name),
        None => "没有正在运行的游戏".to_string(),
    };
    column = column.child(Text::caption(subtitle, theme).tone(Tone::Muted));

    if !model.playing {
        column = column.child(
            EmptyState::new("没有正在运行的游戏", theme).description("开始一个游戏后才能存读档。"),
        );
    } else if !model.saves_supported {
        column = column.child(
            EmptyState::new("该核心不支持即时存档", theme)
                .description("这个 core 没有实现 retro_serialize。"),
        );
    } else {
        let mut list = Column::new().gap(space::XS);
        for row in &model.saves {
            list = list.child(save_slot_row(theme, row, actions));
        }
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(list);
        *scroll = Some(view.state());
        column = column.child(view);
    }
    column
}

/// One save slot: its thumbnail, name, time, and the 存 / 读 / 删 buttons.
fn save_slot_row(theme: &'static dyn Theme, row: &SaveSlotRow, actions: &Actions) -> Row {
    let save = actions.clone();
    let load = actions.clone();
    let delete = actions.clone();
    let slot = row.slot;
    let time = if row.exists {
        format_when(row.modified_ms)
    } else {
        "空".to_string()
    };
    Row::new()
        .align(Align::Center)
        .gap(space::XS)
        .child(save_thumb(theme, row))
        .child(Text::small(format!("槽 {}", slot + 1), theme).grow(1.0))
        .child(Text::caption(time, theme).tone(Tone::Subtle))
        .child(
            Button::ghost("存", theme)
                .mini()
                .on_click(move || save.push(Action::SaveToSlot(slot))),
        )
        .child(
            Button::ghost("读", theme)
                .mini()
                .on_click(move || load.push(Action::LoadFromSlot(slot))),
        )
        .child(
            Button::ghost("删", theme)
                .mini()
                .on_click(move || delete.push(Action::DeleteSlot(slot))),
        )
}

/// A save slot's thumbnail, cropped to fill a small cell.
fn save_thumb(theme: &'static dyn Theme, row: &SaveSlotRow) -> Flex {
    let mut thumb = Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .min_size(48.0, 32.0)
        .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::SM))
        .clip(true);
    if let Some(handle) = row.thumb {
        thumb = thumb.foreground(move |ctx, rect, _state| {
            let destination = cover_fit((handle.width, handle.height), rect);
            ctx.draw_image(handle.texture, destination, None, Paint::default());
        });
    }
    thumb
}

fn screenshots_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let game_id = model.screenshot_game;
    let game_name = game_id
        .and_then(|id| model.games.iter().find(|game| game.id == id))
        .map(|game| game.name.clone());
    let shots: Vec<&ScreenshotRow> = model
        .screenshots
        .iter()
        .filter(|shot| Some(shot.game_id) == game_id)
        .collect();

    let open = actions.clone();
    let toggle = actions.clone();
    let select_label = if model.screenshot_select {
        "完成"
    } else {
        "选择"
    };
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .child(Text::heading("截图收藏", theme).grow(1.0))
                .child(
                    Button::ghost(select_label, theme)
                        .mini()
                        .on_click(move || toggle.push(Action::ToggleScreenshotSelect)),
                )
                .child(
                    Button::ghost("", theme)
                        .mini()
                        .min_size(CARD_ICON_BUTTON, CARD_ICON_BUTTON)
                        .child(SvgIcon::new(
                            IconName::FolderSearch,
                            theme.palette().foreground,
                            CARD_ICON,
                        ))
                        .on_click(move || open.push(Action::OpenScreenshotsFolder)),
                ),
        );
    let subtitle = match &game_name {
        Some(name) => format!("{name} · {} 张", shots.len()),
        None => "没有正在玩的游戏".to_string(),
    };
    column = column.child(Text::caption(subtitle, theme).tone(Tone::Muted));

    if model.screenshot_select {
        let delete = actions.clone();
        let cancel = actions.clone();
        column = column.child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .child(
                    Text::caption(
                        format!("已选 {} 张", model.selected_screenshots.len()),
                        theme,
                    )
                    .grow(1.0),
                )
                .child(
                    Button::destructive("删除选中", theme)
                        .mini()
                        .on_click(move || delete.push(Action::DeleteSelectedScreenshots)),
                )
                .child(
                    Button::ghost("取消", theme)
                        .mini()
                        .on_click(move || cancel.push(Action::ToggleScreenshotSelect)),
                ),
        );
    }

    if shots.is_empty() {
        column = column.child(
            EmptyState::new("还没有截图", theme)
                .description("在游戏里按 F12 截图，⇧F12 直接设为封面。"),
        );
    } else {
        // Only the grid scrolls; the title and the open-folder button stay put.
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(screenshots_grid(theme, model, &shots, actions));
        *scroll = Some(view.state());
        column = column.child(view);
    }
    column
}

/// The screenshots grid, mounted a window at a time like the library grid.
fn screenshots_grid(
    theme: &'static dyn Theme,
    model: &ViewModel,
    shots: &[&ScreenshotRow],
    actions: &Actions,
) -> Column {
    let total = shots.len();
    let mut column = Column::new().gap(0.0).padding(Edges::ZERO);
    if total == 0 {
        return column;
    }
    let columns = model.grid_columns.max(1);
    let rows = total.div_ceil(columns);
    let stride = SHOT_HEIGHT + space::SM;
    let (first, last) = screenshots_grid_window(model);

    let top = first as f32 * stride;
    if top > 0.0 {
        column = column.child(spacer(top));
    }

    let mut grid = Grid::new(vec![Track::Fr(1.0); columns])
        .gap(space::SM)
        .padding(Edges::ZERO);
    for row in first..=last {
        for column_index in 0..columns {
            let index = row * columns + column_index;
            if index >= total {
                break;
            }
            grid = grid.child(shot_card(theme, shots[index], model, actions));
        }
    }
    column = column.child(grid);

    let bottom = rows.saturating_sub(last + 1) as f32 * stride;
    if bottom > 0.0 {
        column = column.child(spacer(bottom));
    }
    column
}

/// One screenshot cell: the thumbnail (a button that previews, or ticks in
/// select mode), the game and time, and the controls.
fn shot_card(
    theme: &'static dyn Theme,
    shot: &ScreenshotRow,
    model: &ViewModel,
    actions: &Actions,
) -> Column {
    let selected = model.selected_screenshots.contains(&shot.id);
    Column::new()
        .gap(space::XXS)
        .padding(Edges::all(space::XXS))
        .min_size(0.0, SHOT_HEIGHT)
        .mouse_filter(MouseFilter::Ignore)
        .dynamic_background(move |state| {
            let fill = if selected {
                theme.palette().selection
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::MD)
        })
        .child(thumbnail(theme, shot, model.screenshot_select, actions))
        .child(
            Text::small(shot.game.as_str(), theme)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(
            Text::caption(format_when(shot.created_at), theme)
                .tone(Tone::Subtle)
                .max_lines(1),
        )
        .child(shot_controls(theme, shot, actions))
}

/// A screenshot thumbnail, cropped to fill the cell. Clicking it previews, or
/// ticks it when the page is in select mode.
fn thumbnail(
    theme: &'static dyn Theme,
    shot: &ScreenshotRow,
    select: bool,
    actions: &Actions,
) -> Flex {
    let click = actions.clone();
    let id = shot.id;
    let mut thumb = Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .min_size(0.0, PLACEHOLDER_HEIGHT)
        .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::SM))
        .clip(true)
        .on_click(move || {
            click.push(if select {
                Action::ToggleScreenshotSelected(id)
            } else {
                Action::PreviewScreenshot(id)
            })
        });
    match shot.thumb {
        Some(handle) => {
            thumb = thumb.foreground(move |ctx, rect, _state| {
                let destination = cover_fit((handle.width, handle.height), rect);
                ctx.draw_image(handle.texture, destination, None, Paint::default());
            });
        }
        None => {
            thumb = thumb.child(SvgIcon::new(IconName::Camera, theme.palette().muted, 20.0));
        }
    }
    thumb
}

/// A screenshot's controls: a "cover" badge or a set-cover button, then reveal
/// and delete.
fn shot_controls(theme: &'static dyn Theme, shot: &ScreenshotRow, actions: &Actions) -> Row {
    let mut row = Row::new().align(Align::Center).gap(space::XXS);
    if shot.is_cover {
        row = row.child(
            Badge::new("封面", theme)
                .fill(theme.palette().selection)
                .text_color(theme.palette().foreground),
        );
    } else {
        let set = actions.clone();
        let id = shot.id;
        row = row.child(icon_button(
            IconName::ImagePlus,
            theme.palette().foreground,
            move || set.push(Action::SetCover(id)),
        ));
    }
    let reveal = actions.clone();
    let reveal_id = shot.id;
    row = row.child(icon_button(
        IconName::FolderSearch,
        theme.palette().foreground,
        move || reveal.push(Action::RevealScreenshot(reveal_id)),
    ));
    let remove = actions.clone();
    let remove_id = shot.id;
    row.child(icon_button(
        IconName::Trash,
        theme.palette().foreground,
        move || remove.push(Action::RequestDelete(Confirm::DeleteScreenshot(remove_id))),
    ))
}

/// The play column while a screenshot is previewed: the picture large, with
/// previous/next and the same controls as the grid.
fn preview_column(
    theme: &'static dyn Theme,
    model: &ViewModel,
    shot: &ScreenshotRow,
    actions: &Actions,
) -> Column {
    let ids: Vec<i64> = model
        .screenshots
        .iter()
        .filter(|candidate| candidate.game_id == shot.game_id)
        .map(|candidate| candidate.id)
        .collect();
    let index = ids.iter().position(|id| *id == shot.id).unwrap_or(0);

    let mut head = Row::new()
        .align(Align::Center)
        .gap(space::SM)
        .child(
            Text::subheading(shot.game.as_str(), theme)
                .grow(1.0)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(Text::caption(format!("{} / {}", index + 1, ids.len()), theme).tone(Tone::Muted));
    if shot.is_cover {
        head = head.child(
            Badge::new("封面", theme)
                .fill(theme.palette().selection)
                .text_color(theme.palette().foreground),
        );
    }

    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore)
        .child(head);
    match shot.thumb {
        Some(handle) => {
            column = column
                .child(FrameImage::new(handle.texture, handle.width, handle.height).grow(1.0));
        }
        None => {
            column = column.child(
                Flex::row()
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .grow(1.0)
                    .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::MD))
                    .child(Text::small("截图缩略图还没加载。", theme).tone(Tone::Muted)),
            );
        }
    }

    let previous = actions.clone();
    let next = actions.clone();
    column = column.child(
        Row::new()
            .align(Align::Center)
            .justify(Justify::Center)
            .gap(space::SM)
            .child(
                Button::ghost("", theme)
                    .child(SvgIcon::new(
                        IconName::ChevronLeft,
                        theme.palette().foreground,
                        14.0,
                    ))
                    .on_click(move || previous.push(Action::StepPreview(-1))),
            )
            .child(Text::caption(format_when(shot.created_at), theme).tone(Tone::Subtle))
            .child(
                Button::ghost("", theme)
                    .child(SvgIcon::new(
                        IconName::ChevronRight,
                        theme.palette().foreground,
                        14.0,
                    ))
                    .on_click(move || next.push(Action::StepPreview(1))),
            ),
    );

    let mut controls = Row::new().gap(space::SM);
    if !shot.is_cover {
        let set = actions.clone();
        let id = shot.id;
        controls = controls.child(
            Button::secondary("设为封面", theme).on_click(move || set.push(Action::SetCover(id))),
        );
    }
    let reveal = actions.clone();
    let reveal_id = shot.id;
    controls = controls.child(
        Button::ghost("在访达中显示", theme)
            .on_click(move || reveal.push(Action::RevealScreenshot(reveal_id))),
    );
    let remove = actions.clone();
    let remove_id = shot.id;
    controls = controls.child(Button::destructive("删除", theme).on_click(move || {
        remove.push(Action::RequestDelete(Confirm::DeleteScreenshot(remove_id)))
    }));
    let close = actions.clone();
    controls = controls
        .child(Button::secondary("关闭", theme).on_click(move || close.push(Action::ClosePreview)));
    column.child(controls)
}

/// A short "how long ago" for a screenshot caption.
fn format_when(created_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let seconds = ((now - created_ms) / 1000).max(0);
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else {
        format!("{} 天前", seconds / 86_400)
    }
}

/// The bottom status line: the last app message, plus the save hotkeys.
fn status_bar(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    let status = if model.status.is_empty() {
        "就绪"
    } else {
        model.status.as_str()
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
                        .tone(Tone::Muted)
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

fn settings_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .padding(Edges::all(space::MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title("设置", theme));
    let mut body = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);

    // Library folders the scanner walks.
    let mut dirs = Column::new().gap(space::XS);
    if model.library_dirs.is_empty() {
        dirs = dirs.child(Text::small("还没有扫描目录。", theme).tone(Tone::Muted));
    } else {
        for (index, dir) in model.library_dirs.iter().enumerate() {
            let actions = actions.clone();
            dirs = dirs.child(
                Row::new()
                    .gap(space::SM)
                    .child(
                        Text::small(dir.as_str(), theme)
                            .grow(1.0)
                            .max_lines(1)
                            .ellipsis(true),
                    )
                    .child(
                        Button::ghost("移除", theme)
                            .on_click(move || actions.push(Action::RemoveLibraryDir(index))),
                    ),
            );
        }
    }
    let add = actions.clone();
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("游戏目录", theme))
            .child(dirs)
            .child(
                Button::secondary("添加游戏目录…", theme)
                    .on_click(move || add.push(Action::OpenRom)),
            ),
    );

    // One core pick per console. The selected one is the core that would run
    // the console now (the saved pick, or the manifest default).
    let mut cores = Column::new().gap(space::SM);
    let mut any_core = false;
    for system in cgb_systems::SYSTEMS {
        let mut group = Column::new().gap(space::XS);
        let mut any = false;
        for (index, core) in model.cores.iter().enumerate() {
            if core.system != *system {
                continue;
            }
            any = true;
            any_core = true;
            let actions = actions.clone();
            let button = if core.selected {
                Button::primary(format!("{}（当前）", core.name), theme)
            } else {
                Button::ghost(core.name.clone(), theme)
            };
            group = group.child(button.on_click(move || actions.push(Action::SelectCore(index))));
        }
        if any {
            cores = cores
                .child(Text::small(system.name(), theme).tone(Tone::Muted))
                .child(group);
        }
    }
    if !any_core {
        cores =
            cores.child(Text::small("没有可用核心（见 cores.json）。", theme).tone(Tone::Muted));
    }
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("模拟器核心", theme))
            .child(cores),
    );

    // The game-picture post-process preset.
    let mut shaders = Row::new().gap(space::XS);
    for kind in ShaderKind::ALL {
        let actions = actions.clone();
        let button = if model.shader == kind {
            Button::primary(kind.label(), theme)
        } else {
            Button::ghost(kind.label(), theme)
        };
        shaders = shaders.child(
            button
                .mini()
                .on_click(move || actions.push(Action::SetShader(kind))),
        );
    }
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("画面效果", theme))
            .child(shaders)
            .child(Text::caption("对游戏画面做后处理，不影响界面。", theme).tone(Tone::Subtle)),
    );

    // The running core's own options, when a game has been loaded.
    if !model.core_options.is_empty() {
        let mut options = Column::new().gap(space::XS);
        for (index, option) in model.core_options.iter().enumerate() {
            let previous = actions.clone();
            let next = actions.clone();
            options = options.child(
                Row::new()
                    .align(Align::Center)
                    .gap(space::XS)
                    .child(
                        Text::small(option.label.as_str(), theme)
                            .grow(1.0)
                            .max_lines(1)
                            .ellipsis(true),
                    )
                    .child(
                        Button::ghost("‹", theme)
                            .mini()
                            .on_click(move || previous.push(Action::CycleCoreOption(index, -1))),
                    )
                    .child(Text::caption(option.value.as_str(), theme).tone(Tone::Muted))
                    .child(
                        Button::ghost("›", theme)
                            .mini()
                            .on_click(move || next.push(Action::CycleCoreOption(index, 1))),
                    ),
            );
        }
        body = body.child(
            Card::new(theme)
                .gap(space::SM)
                .padding(Edges::all(space::SM))
                .child(Text::subheading("核心选项", theme))
                .child(options),
        );
    }

    // Keyboard bindings, read-only for now.
    let mut bindings = Column::new().gap(space::XS);
    if model.bindings.is_empty() {
        bindings = bindings.child(Text::small("没有绑定。", theme).tone(Tone::Muted));
    } else {
        for row in &model.bindings {
            bindings = bindings.child(Text::small(
                format!("{}   —   {}", row.button, row.keys),
                theme,
            ));
        }
    }
    let title = if model.bindings_system.is_empty() {
        "按键".to_string()
    } else {
        format!("按键（{}）", model.bindings_system)
    };
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading(title, theme))
            .child(bindings)
            .child(
                Text::caption("键盘按机种分别保存；手柄走 gilrs 自动映射。", theme)
                    .tone(Tone::Subtle),
            ),
    );

    // The core's own input descriptors, when a game has been loaded: mGBA's
    // shoulder buttons, an arcade stick's buttons, and so on.
    if !model.core_inputs.is_empty() {
        let mut inputs = Column::new().gap(space::XS);
        for row in &model.core_inputs {
            inputs = inputs.child(Text::small(
                format!(
                    "端口 {} · 设备 {} · 索引 {} · id {}   —   {}",
                    row.port, row.device, row.index, row.id, row.description
                ),
                theme,
            ));
        }
        body = body.child(
            Card::new(theme)
                .gap(space::SM)
                .padding(Edges::all(space::SM))
                .child(Text::subheading("核心输入", theme))
                .child(inputs),
        );
    }

    // Only the settings bodies scroll; the title stays put.
    let view = ScrollView::new(theme)
        .grow(1.0)
        .scrollbar(false)
        .child(body);
    *scroll = Some(view.state());
    column = column.child(view);

    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BindingRow, CoreRow, FrameHandle, GameRow};
    use cgb_systems::SystemId;
    use draw_core::{InputEvent, PointerButton, Size, Vec2};
    use draw_render::{DrawCommand, PaintContext, TextureId};
    use draw_theme::{default_theme, Mode};

    /// Build, lay out (running the scroll sync) and paint the shell. Returns the
    /// mounted tree so a test can also route input at it.
    fn laid_out(model: &ViewModel, actions: &Actions) -> (SceneTree, draw_render::DrawList) {
        let theme = default_theme(Mode::Dark);
        let width = Rc::new(Cell::new(model.middle_width));
        let (mut tree, mut scroll) = build(
            theme,
            model,
            actions,
            &width,
            &NodeRef::new(),
            &Cell::new((0, 0)),
        );
        let viewport = draw_core::ViewportSize::new(Size::new(1100.0, 760.0));
        draw_ui::layout(&mut tree, viewport);
        tree.update();
        if let Some(scroll) = scroll.as_mut() {
            if scroll.sync(&mut tree) {
                draw_ui::layout(&mut tree, viewport);
                tree.update();
            }
        }
        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        (tree, ctx.into_draw_list())
    }

    fn text_position(list: &draw_render::DrawList, needle: &str) -> Vec2 {
        list.commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text {needle:?}"))
    }

    /// A minimal library row for the view tests.
    fn game_row(name: &str, path: &str) -> GameRow {
        GameRow {
            id: 0,
            name: name.to_string(),
            file_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            system: SystemId::Nes,
            path: path.to_string(),
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

    /// A card is a click target: a press and release inside it starts the game.
    #[test]
    fn clicking_a_card_plays_it() {
        let actions = Actions::default();
        let model = ViewModel {
            games: vec![game_row("Game 0", "/roms/game0.nes")],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let point = text_position(&list, "Game 0");

        let hit = draw_ui::hit_test(&tree, point).expect("something is under the card");
        assert!(
            draw_ui::is_interactive(&tree, hit),
            "the card is interactive"
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        );
        assert_eq!(actions.drain(), vec![Action::Play(0)]);
    }

    /// Press and release at `point`, the way a mouse click arrives.
    fn click(tree: &mut SceneTree, point: Vec2) {
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
            draw_ui::handle_input(tree, &event);
        }
    }

    fn one_game() -> ViewModel {
        ViewModel {
            games: vec![game_row("Game 0", "/roms/game0.nes")],
            ..ViewModel::default()
        }
    }

    /// Lay out a single card cover (no shell chrome around it), so its two
    /// icon buttons can be located by the lines their SVGs draw.
    fn isolated_cover(game: &GameRow, actions: &Actions) -> (SceneTree, draw_render::DrawList) {
        let theme = default_theme(Mode::Dark);
        let mut tree = SceneTree::new();
        let root = tree.root();
        tree.add_child(
            root,
            Flex::column()
                .gap(0.0)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(cover(theme, game, 0, actions)),
        );
        draw_ui::layout(
            &mut tree,
            draw_core::ViewportSize::new(Size::new(320.0, 240.0)),
        );
        tree.update();
        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        (tree, ctx.into_draw_list())
    }

    /// The centres of the two SVG icons in a cover, split left/right by their
    /// drawn lines: the pin is left, the delete is right.
    /// Cluster the icon line endpoints into `n` groups by the widest x gaps
    /// and return each group's centre, left to right. The card's controls are
    /// pencil, tag, pin, delete in that order.
    fn icon_centres(list: &draw_render::DrawList, n: usize) -> Vec<Vec2> {
        let mut points: Vec<Vec2> = Vec::new();
        for command in list.commands() {
            if let DrawCommand::Line { from, to, .. } = command {
                points.push(*from);
                points.push(*to);
            }
        }
        assert!(!points.is_empty(), "the cover drew no icon lines");
        points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        let mut gaps: Vec<(usize, f32)> = (1..points.len())
            .map(|index| (index, points[index].x - points[index - 1].x))
            .collect();
        gaps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let mut cuts: Vec<usize> = gaps
            .into_iter()
            .take(n - 1)
            .map(|(index, _)| index)
            .collect();
        cuts.sort_unstable();
        let mut groups: Vec<&[Vec2]> = Vec::new();
        let mut start = 0;
        for cut in cuts {
            groups.push(&points[start..cut]);
            start = cut;
        }
        groups.push(&points[start..]);
        groups
            .iter()
            .map(|group| {
                let count = group.len() as f32;
                Vec2::new(
                    group.iter().map(|p| p.x).sum::<f32>() / count,
                    group.iter().map(|p| p.y).sum::<f32>() / count,
                )
            })
            .collect()
    }

    fn one_game_row() -> GameRow {
        one_game().games.remove(0)
    }

    /// The pin icon is its own button and toggles the pin.
    #[test]
    fn clicking_pin_toggles_it() {
        let actions = Actions::default();
        let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
        let centres = icon_centres(&list, 4);
        click(&mut tree, centres[2]);
        assert_eq!(actions.drain(), vec![Action::TogglePin(0)]);
    }

    /// The pencil and tag icons start a name / tag edit.
    #[test]
    fn clicking_rename_and_tag_start_edits() {
        let actions = Actions::default();
        let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
        let centres = icon_centres(&list, 4);
        click(&mut tree, centres[0]);
        assert_eq!(actions.drain(), vec![Action::StartRename(0)]);
        click(&mut tree, centres[1]);
        assert_eq!(actions.drain(), vec![Action::StartTagEdit(0)]);
    }

    /// The delete icon is its own button: it deletes, it does not start the
    /// game (the nearest callback wins over the card's play callback).
    #[test]
    fn clicking_delete_deletes_without_playing() {
        let actions = Actions::default();
        let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
        let centres = icon_centres(&list, 4);
        click(&mut tree, centres[3]);
        assert_eq!(
            actions.drain(),
            vec![Action::RequestDelete(Confirm::DeleteGame(0))]
        );
    }

    /// The sort bar offers every key and the direction toggle.
    #[test]
    fn the_sort_bar_switches_the_key() {
        let actions = Actions::default();
        let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
        let point = text_position(&list, "大小");
        click(&mut tree, point);
        assert_eq!(actions.drain(), vec![Action::Sort(SortKey::Size)]);
    }

    /// Covers are a stable colour per path, so a game keeps its colour between
    /// runs; different games get different colours.
    #[test]
    fn cover_colours_are_stable_and_varied() {
        assert_eq!(cover_color("/roms/a.nes"), cover_color("/roms/a.nes"));
        assert_ne!(cover_color("/roms/a.nes"), cover_color("/roms/b.nes"));
    }

    /// A card's tag line shows a few words then a count.
    #[test]
    fn tags_label_lists_a_few_words() {
        let mut game = game_row("Game 0", "/roms/game0.nes");
        game.tags = [
            "RPG".to_string(),
            "Action".to_string(),
            "Long".to_string(),
            "Extra".to_string(),
        ]
        .to_vec();
        assert_eq!(tags_label(&game), "#RPG #Action #Long +1");
        game.tags.truncate(2);
        assert_eq!(tags_label(&game), "#RPG #Action");
    }

    /// A card with a cover texture draws it (the only image when no game is
    /// playing).
    #[test]
    fn a_card_with_a_cover_draws_its_texture() {
        let mut game = game_row("Game 0", "/roms/game0.nes");
        game.cover = Some(FrameHandle {
            texture: TextureId::new(0x1000),
            width: 256,
            height: 240,
        });
        let model = ViewModel {
            games: vec![game],
            ..ViewModel::default()
        };
        let list = paint(&model);
        assert!(list
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::DrawImage { .. })));
    }

    /// The meta line reads the file size and the play count/time.
    #[test]
    fn card_meta_reads_size_and_play_time() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(format_duration(45), "45 秒");
        assert_eq!(format_duration(90), "1 分");
        assert_eq!(format_duration(3660), "1 时 1 分");

        let mut game = game_row("Game 0", "/roms/game0.nes");
        assert!(meta_label(&game).contains("未玩过"));
        game.play_count = 3;
        game.play_seconds = 120;
        game.size = 2048;
        let meta = meta_label(&game);
        assert!(meta.contains("2 KB"), "{meta}");
        assert!(meta.contains("玩过 3 次"), "{meta}");
        assert!(meta.contains("2 分"), "{meta}");
    }

    fn one_shot() -> ScreenshotRow {
        ScreenshotRow {
            id: 7,
            game_id: 0,
            game: "Game 0".to_string(),
            created_at: 0,
            is_cover: true,
            thumb: Some(FrameHandle {
                texture: TextureId::new(0x1_0000),
                width: 256,
                height: 240,
            }),
        }
    }

    /// The screenshots section lists the game, its shot count, the cover badge
    /// and the thumbnail image.
    #[test]
    fn the_screenshots_page_lists_the_game_and_its_shots() {
        let model = ViewModel {
            section: Section::Screenshots,
            screenshot_game: Some(0),
            games: vec![game_row("Game 0", "/roms/game0.nes")],
            screenshots: vec![one_shot()],
            ..ViewModel::default()
        };
        let list = paint(&model);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("截图收藏"), "the page title");
        assert!(has("Game 0"), "the game name");
        assert!(has("1 张"), "the count");
        assert!(has("封面"), "the cover flag");
        assert!(
            list.commands()
                .iter()
                .any(|command| matches!(command, DrawCommand::DrawImage { .. })),
            "the thumbnail is drawn"
        );
    }

    /// The play column shows the previewed screenshot and its index.
    #[test]
    fn the_preview_column_shows_the_picture_and_its_index() {
        let model = ViewModel {
            preview: Some(7),
            games: vec![game_row("Game 0", "/roms/game0.nes")],
            screenshots: vec![one_shot()],
            ..ViewModel::default()
        };
        let list = paint(&model);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("1 / 1"), "the position");
        assert!(
            list.commands()
                .iter()
                .any(|command| matches!(command, DrawCommand::DrawImage { .. })),
            "the preview is drawn"
        );
    }

    /// The card's screenshot count is its own button that opens the section.
    #[test]
    fn the_card_screenshot_count_opens_the_section() {
        let actions = Actions::default();
        let mut game = game_row("Game 0", "/roms/game0.nes");
        game.screenshots = 2;
        let model = ViewModel {
            games: vec![game],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let point = text_position(&list, "2");
        click(&mut tree, point);
        assert_eq!(actions.drain(), vec![Action::ShowScreenshots(0)]);
    }

    /// The rail has a screenshots entry that switches the section.
    #[test]
    fn the_rail_offers_the_screenshots_section() {
        let actions = Actions::default();
        let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
        let point = text_position(&list, "截图");
        click(&mut tree, point);
        assert!(actions
            .drain()
            .contains(&Action::Show(Section::Screenshots)));
    }

    /// A pending delete shows a confirmation bar whose buttons confirm or
    /// cancel it.
    #[test]
    fn a_pending_delete_shows_a_confirmation_bar() {
        let actions = Actions::default();
        let model = ViewModel {
            confirm: Some(Confirm::DeleteGame(0)),
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text == needle)
            })
        };
        assert!(
            has(Confirm::DeleteGame(0).message()),
            "the confirmation asks the question"
        );

        let confirm = text_position(&list, "删除");
        click(&mut tree, confirm);
        assert_eq!(actions.drain(), vec![Action::ConfirmDelete]);

        let cancel = text_position(&list, "取消");
        click(&mut tree, cancel);
        assert_eq!(actions.drain(), vec![Action::CancelDelete]);
    }

    /// The edit bar shows the text with a caret, and its buttons commit or
    /// cancel the edit.
    #[test]
    fn the_edit_bar_shows_the_field_and_commits() {
        let actions = Actions::default();
        let model = ViewModel {
            editing: Some(EditState {
                game_id: 0,
                kind: EditKind::Name,
                text: "New Name".to_string(),
                caret: 3,
            }),
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text == needle)
            })
        };
        assert!(has("改名"), "the bar is labelled");
        assert!(has("New"), "the text before the caret");
        assert!(has("|"), "the caret");
        assert!(has("Name"), "the text after the caret");

        click(&mut tree, text_position(&list, "保存"));
        assert_eq!(actions.drain(), vec![Action::CommitEdit]);
        click(&mut tree, text_position(&list, "取消"));
        assert_eq!(actions.drain(), vec![Action::CancelEdit]);
    }

    /// The search bar opens the field, and shows the current query.
    #[test]
    fn the_search_bar_starts_and_reflects_the_query() {
        let actions = Actions::default();
        let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
        click(&mut tree, text_position(&list, "搜索…"));
        assert_eq!(actions.drain(), vec![Action::StartSearch]);

        let model = ViewModel {
            search: "mario".to_string(),
            ..ViewModel::default()
        };
        let list = paint(&model);
        assert!(list.commands().iter().any(|command| matches!(command,
            DrawCommand::DrawText { text, .. } if text.contains("mario"))));
    }

    /// The saves section lists the slots and its buttons emit the slot actions.
    #[test]
    fn the_saves_page_lists_slots_and_emits_actions() {
        let actions = Actions::default();
        let model = ViewModel {
            section: Section::Saves,
            playing: true,
            core_name: "Mesen".to_string(),
            saves_supported: true,
            saves: vec![
                SaveSlotRow {
                    slot: 0,
                    exists: true,
                    modified_ms: 0,
                    thumb: None,
                },
                SaveSlotRow {
                    slot: 1,
                    exists: false,
                    modified_ms: 0,
                    thumb: None,
                },
            ],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("槽 1"));
        assert!(has("槽 2"));

        click(&mut tree, text_position(&list, "存"));
        assert_eq!(actions.drain(), vec![Action::SaveToSlot(0)]);
        click(&mut tree, text_position(&list, "读"));
        assert_eq!(actions.drain(), vec![Action::LoadFromSlot(0)]);
        click(&mut tree, text_position(&list, "删"));
        assert_eq!(actions.drain(), vec![Action::DeleteSlot(0)]);
    }

    /// The rail has a save section entry.
    #[test]
    fn the_rail_offers_the_saves_section() {
        let actions = Actions::default();
        let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
        click(&mut tree, text_position(&list, "存档"));
        assert!(actions.drain().contains(&Action::Show(Section::Saves)));
    }

    /// The cheats page lists cheats, toggles one, and offers the import.
    #[test]
    fn the_cheats_page_lists_and_toggles() {
        let actions = Actions::default();
        let model = ViewModel {
            section: Section::Cheats,
            playing: true,
            core_name: "Mesen".to_string(),
            cheats: vec![
                CheatRow {
                    desc: "Infinite Lives".to_string(),
                    code: "AAAA".to_string(),
                    enabled: true,
                },
                CheatRow {
                    desc: "Max Coins".to_string(),
                    code: "BBBB".to_string(),
                    enabled: false,
                },
            ],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("Infinite Lives"));
        assert!(has("AAAA"));

        click(&mut tree, text_position(&list, "开"));
        assert_eq!(actions.drain(), vec![Action::ToggleCheat(0)]);
        click(&mut tree, text_position(&list, "导入 .cht…"));
        assert_eq!(actions.drain(), vec![Action::ImportCheats]);
    }

    /// The rail has a cheats entry.
    #[test]
    fn the_rail_offers_the_cheats_section() {
        let actions = Actions::default();
        let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
        click(&mut tree, text_position(&list, "金手指"));
        assert!(actions.drain().contains(&Action::Show(Section::Cheats)));
    }

    /// The rail is the only way to change what the middle column shows, so it
    /// has to be clickable too (icon + label inside a clickable cell).
    #[test]
    fn clicking_the_rail_switches_section() {
        let actions = Actions::default();
        let model = ViewModel::default();
        let (mut tree, list) = laid_out(&model, &actions);
        let point = text_position(&list, "设置");
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        );
        assert!(actions.drain().contains(&Action::Show(Section::Settings)));
    }

    fn paint(model: &ViewModel) -> draw_render::DrawList {
        let actions = Actions::default();
        laid_out(model, &actions).1
    }

    /// The play column is always mounted, so a loaded frame emits the image
    /// command the wgpu backend turns into a texture blit — even while the
    /// middle column shows the library.
    #[test]
    fn the_play_column_emits_a_draw_image() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel {
            playing: true,
            frame: Some(FrameHandle {
                texture: TextureId::new(1),
                width: 256,
                height: 240,
            }),
            ..ViewModel::default()
        };

        let width = Rc::new(Cell::new(model.middle_width));
        let (mut tree, _) = build(
            theme,
            &model,
            &actions,
            &width,
            &NodeRef::new(),
            &Cell::new((0, 0)),
        );
        draw_ui::layout(
            &mut tree,
            draw_core::ViewportSize::new(Size::new(1100.0, 760.0)),
        );
        tree.update();

        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        let list = ctx.into_draw_list();

        let destination = list.commands().iter().find_map(|command| match command {
            DrawCommand::DrawImage { destination, .. } => Some(*destination),
            _ => None,
        });
        let destination = destination.expect("the play column draws the framebuffer");
        // One scale for both axes: aspect ratio preserved (the fit may be
        // fractional, so the scale is not necessarily a whole number).
        let scale_x = destination.size.width / 256.0;
        let scale_y = destination.size.height / 240.0;
        assert!(scale_x > 0.0);
        assert!((scale_x - scale_y).abs() < 0.01, "{scale_x} vs {scale_y}");
    }

    /// Save/load results arrive as `ViewModel::status`; the shared shell must
    /// paint it, not swallow it.
    #[test]
    fn the_status_line_is_painted() {
        let model = ViewModel {
            status: "已存档（槽位 0）".to_string(),
            ..ViewModel::default()
        };
        let list = paint(&model);
        assert!(list.commands().iter().any(|command| matches!(command,
            DrawCommand::DrawText { text, .. } if text.contains("已存档"))));
    }

    /// The settings page must actually show the scanned folders and the core
    /// choices, not the old placeholder text.
    #[test]
    fn the_settings_shader_presets_emit_actions() {
        let actions = Actions::default();
        let model = ViewModel {
            section: Section::Settings,
            shader: ShaderKind::Off,
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        click(&mut tree, text_position(&list, "CRT"));
        assert_eq!(actions.drain(), vec![Action::SetShader(ShaderKind::Crt)]);
    }

    #[test]
    fn the_settings_core_options_cycle() {
        let actions = Actions::default();
        let model = ViewModel {
            section: Section::Settings,
            core_options: vec![crate::model::CoreOptionRow {
                key: "region".to_string(),
                label: "Region".to_string(),
                values: vec![
                    ("auto".to_string(), "Auto".to_string()),
                    ("ntsc".to_string(), "NTSC".to_string()),
                ],
                value: "auto".to_string(),
            }],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("Region"));
        click(&mut tree, text_position(&list, "›"));
        assert_eq!(actions.drain(), vec![Action::CycleCoreOption(0, 1)]);
    }

    #[test]
    fn the_settings_page_lists_dirs_and_cores() {
        let model = ViewModel {
            section: Section::Settings,
            library_dirs: vec!["/roms/nes".to_string()],
            cores: vec![
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
            ],
            bindings: vec![BindingRow {
                button: "A".to_string(),
                keys: "X / K".to_string(),
            }],
            ..ViewModel::default()
        };
        let list = paint(&model);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("/roms/nes"), "the folder is listed");
        assert!(has("Nestopia"), "every core for the console is offered");
        assert!(has("Mesen"), "the selected core is shown");
        assert!(has("X / K"), "the binding is shown");
    }

    /// The grid mounts a window of rows, not the whole library.
    /// The number of grid columns steps 2 / 3 / 4 as the middle column widens,
    /// and never leaves that range.
    #[test]
    fn library_columns_steps_with_the_middle_width() {
        assert_eq!(library_columns(MIDDLE_MIN_WIDTH), MIN_LIBRARY_COLUMNS);
        assert_eq!(library_columns(MIDDLE_MAX_WIDTH), MAX_LIBRARY_COLUMNS);
        assert!(
            (MIN_LIBRARY_COLUMNS..=MAX_LIBRARY_COLUMNS).contains(&library_columns(400.0)),
            "a mid width picks a valid count"
        );
        assert!(library_columns(400.0) <= library_columns(600.0));
    }

    #[test]
    fn visible_rows_windows_the_grid() {
        // 10 rows, 100px stride, 250px viewport: the visible rows plus slack.
        assert_eq!(visible_rows(0.0, 250.0, 10, 100.0), (0, 4));
        assert_eq!(visible_rows(500.0, 250.0, 10, 100.0), (5, 9));
        // Past the end clamps to the last row.
        assert_eq!(visible_rows(5000.0, 250.0, 10, 100.0), (9, 9));
        // Nothing to show.
        assert_eq!(visible_rows(0.0, 250.0, 0, 100.0), (0, 0));
    }

    /// The mounted window only changes when a row boundary is crossed, so a
    /// scroll smaller than one row needs no rebuild.
    #[test]
    fn the_grid_window_only_changes_when_a_row_is_crossed() {
        let mut model = ViewModel {
            games: (0..40)
                .map(|index| game_row(&format!("Game {index}"), &format!("/roms/game{index}.nes")))
                .collect(),
            grid_viewport: 400.0,
            ..ViewModel::default()
        };
        let start = grid_window(&model);
        // A few pixels into the same row: same window.
        model.grid_offset = 20.0;
        assert_eq!(grid_window(&model), start);
        // Past a row boundary (`CARD_HEIGHT + space::SM = 192`): new window.
        model.grid_offset = 200.0;
        assert_ne!(grid_window(&model), start);
        assert_eq!(grid_window(&model), (1, 5));
    }

    /// The library is a grid, not a list: the first `grid_columns` cells share
    /// a row (increasing x, same baseline) and the next one wraps to a new row
    /// below.
    #[test]
    fn the_library_page_lays_games_out_in_a_grid() {
        let columns = ViewModel::default().grid_columns;
        let games: Vec<GameRow> = (0..columns + 1)
            .map(|index| game_row(&format!("Game {index}"), &format!("/roms/game{index}.nes")))
            .collect();
        let model = ViewModel {
            games,
            ..ViewModel::default()
        };
        let list = paint(&model);
        let position = |needle: &str| {
            list.commands().iter().find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
                _ => None,
            })
        };

        let first = position("Game 0").expect("the first card paints its title");
        let second = position("Game 1").expect("the second card paints its title");
        let wrapped =
            position(&format!("Game {columns}")).expect("the wrapped card paints its title");

        assert!(
            (first.y - second.y).abs() < 0.5,
            "the first columns share a row: {first:?} vs {second:?}"
        );
        assert!(second.x > first.x, "the next column is to the right");
        assert!(wrapped.y > first.y, "the next row is below the first");
    }

    /// The shell is three columns: the library grid is in the middle column,
    /// and the console column is entirely to its right.
    #[test]
    fn the_shell_is_three_columns() {
        let model = ViewModel {
            games: vec![game_row("Game 0", "/roms/game0.nes")],
            playing: true,
            frame: Some(FrameHandle {
                texture: TextureId::new(1),
                width: 256,
                height: 240,
            }),
            ..ViewModel::default()
        };
        let list = paint(&model);
        let cover_x = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == "Game 0" => {
                    Some(position.x)
                }
                _ => None,
            })
            .expect("the card paints the name");
        let image_left = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawImage { destination, .. } => Some(destination.left()),
                _ => None,
            })
            .expect("the console paints the frame");

        let middle_end = RAIL_WIDTH + 320.0;
        assert!(
            cover_x < middle_end,
            "the grid sits in the middle column: {cover_x}"
        );
        assert!(
            image_left >= middle_end,
            "the console is right of the middle column: {image_left} < {middle_end}"
        );
    }

    /// The settings bodies scroll when they are taller than the middle column.
    #[test]
    fn the_settings_page_scrolls_when_it_overflows() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let cores: Vec<CoreRow> = (0..40)
            .map(|index| CoreRow {
                key: format!("core{index}"),
                name: format!("Core {index}"),
                system: SystemId::Nes,
                selected: index == 0,
            })
            .collect();
        let model = ViewModel {
            section: Section::Settings,
            cores,
            ..ViewModel::default()
        };
        let width = Rc::new(Cell::new(model.middle_width));
        let (mut tree, mut scroll) = build(
            theme,
            &model,
            &actions,
            &width,
            &NodeRef::new(),
            &Cell::new((0, 0)),
        );
        let viewport = draw_core::ViewportSize::new(Size::new(1100.0, 760.0));
        draw_ui::layout(&mut tree, viewport);
        tree.update();
        let scroll = scroll.as_mut().expect("the settings page has a ScrollView");
        scroll.sync(&mut tree);
        assert!(
            scroll.content_height() > scroll.viewport_height(),
            "the bodies overflow, so they scroll: {} vs {}",
            scroll.content_height(),
            scroll.viewport_height()
        );
    }
}
