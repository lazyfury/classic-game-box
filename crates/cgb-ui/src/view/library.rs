//! The game library page: the totals and console filter, the sort and search
//! controls, and the virtualized grid of cover cards.

use igui::igui_components::{
    Button, Column, Component, Divider, EmptyState, Flex, Grid, NodeRef, Row, ScrollView,
    ScrollViewState, Text,
};
use igui::igui_core::{Color, Cursor, Edges};
use igui::igui_render::Paint;
use igui::igui_theme::radius::MD;
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle, Track};

use crate::frame::cover_fit;
use crate::icons::{Icon as SvgIcon, IconName};
use crate::model::{Action, Confirm, EditKind, EditState, GameRow, SortKey, ViewModel};

use super::components::{
    chip, chip_bar, chip_group, compact_button, grid_viewport, icon_button, spacer, text_field,
    window_for,
};
use super::Actions;
use super::{media, CARD_HEIGHT, CARD_ICON, CARD_ICON_BUTTON, PLACEHOLDER_HEIGHT};

pub(super) fn library_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(
        Row::new()
            .justify(Justify::SpaceBetween)
            .align(Align::Center)
            .child(Text::heading("游戏库", theme))
            .child(
                Text::caption(format!("共 {} 个游戏", model.total_games), theme).tone(Tone::Muted),
            ),
    );
    column = column.child(stats_bar(theme, model, actions));
    column = column.child(sort_bar(theme, model, actions));
    column = column.child(Divider::horizontal(theme));
    column = column.child(search_bar(theme, model, actions));
    if let Some(edit) = &model.editing {
        if matches!(edit.kind, EditKind::Name | EditKind::Tags) {
            column = column.child(edit_bar(theme, edit, actions));
        }
    }

    if model.games.is_empty() {
        let empty = if !model.search.is_empty() {
            EmptyState::new("没有匹配的游戏", theme).description("换个关键词，或用 #标签 过滤。")
        } else if model.system_filter.is_some() {
            EmptyState::new("该机种还没有游戏", theme).description("点“全部”看整个游戏库。")
        } else {
            EmptyState::new("还没有游戏", theme).description("把 ROM 拖进来，或点“添加游戏文件…”。")
        };
        column = column.child(empty);
    } else {
        // Only the grid scrolls; the title and the add buttons stay put.
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(library_grid(theme, model, actions));
        *scroll = Some(view.state());
        column = column.child(view);
    }

    let add_files = actions.clone();
    let add_dir = actions.clone();
    column = column.child(
        Row::new()
            .gap(space::SM)
            .child(
                Button::secondary("添加游戏文件…", theme)
                    .on_click(move |_tree, _id| add_files.push(Action::AddGames)),
            )
            .child(
                Button::ghost("打开游戏库…", theme)
                    .on_click(move |_tree, _id| add_dir.push(Action::SwitchLibrary)),
            ),
    );
    column
}

/// The library's console filter: a chip per console present (with its count),
/// then an “全部” chip. The total is shown by the page title, not here.
pub(super) fn stats_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Flex {
    let mut filter_chips = chip_group();
    let all = actions.clone();
    filter_chips = filter_chips.child(
        chip(theme, "全部", model.system_filter.is_none())
            .on_click(move |_tree, _id| all.push(Action::FilterSystem(None))),
    );
    for tally in &model.system_counts {
        let filter = actions.clone();
        let system = tally.system;
        let label = format!("{} {}", system.short(), tally.count);
        filter_chips = filter_chips.child(
            chip(theme, &label, model.system_filter == Some(system))
                .on_click(move |_tree, _id| filter.push(Action::FilterSystem(Some(system)))),
        );
    }
    chip_bar(theme, "按模拟器筛选", filter_chips)
}

/// The library's sort controls: a chip per key, then a direction toggle.
/// Pinned games are always on top, so the key only orders within the groups.
pub(super) fn sort_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Flex {
    let mut sort_chips = chip_group();
    for key in SortKey::ALL {
        let actions = actions.clone();
        sort_chips = sort_chips.child(
            chip(theme, key.label(), model.sort == key)
                .on_click(move |_tree, _id| actions.push(Action::Sort(key))),
        );
    }
    let toggle = actions.clone();
    let arrow = if model.sort_desc {
        IconName::ArrowDown
    } else {
        IconName::ArrowUp
    };
    let dir_node = NodeRef::new();
    actions.tip(&dir_node, "切换升降序");
    sort_chips = sort_chips.child(
        Button::ghost("", theme)
            .mini()
            .child(SvgIcon::new(arrow, theme.palette().foreground, CARD_ICON))
            .on_click(move |_tree, _id| toggle.push(Action::ToggleSortOrder))
            .ref_(&dir_node),
    );
    chip_bar(theme, "排序", sort_chips)
}

/// The search row: a button that opens the field, or the field while typing,
/// with a clear control when a query is set.
pub(super) fn search_bar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Row {
    let mut row = Row::new().align(Align::Center).gap(space::XS);
    if let Some(edit) = &model.editing {
        if edit.kind == EditKind::Search {
            let done = actions.clone();
            let clear = actions.clone();
            return row
                .child(text_field(theme, actions, "名称 / #标签…").grow(1.0))
                .child(
                    Button::primary("完成", theme)
                        .mini()
                        .on_click(move |_tree, _id| done.push(Action::CommitEdit)),
                )
                .child(
                    Button::ghost("清除", theme)
                        .mini()
                        .on_click(move |_tree, _id| clear.push(Action::ClearSearch)),
                );
        }
    }
    let start = actions.clone();
    let label = if model.search.is_empty() {
        "搜索名称 / #标签…".to_string()
    } else {
        format!("搜索：{}", model.search)
    };
    row = row.child(
        Button::new("", theme)
            .child(
                Row::new()
                    .grow(1.0)
                    .shrink(1.0)
                    .align(Align::Start)
                    .justify(Justify::Start)
                    .child(Text::caption(label, theme)),
            )
            .grow(1.0)
            .on_click(move |_tree, _id| start.push(Action::StartSearch)),
    );
    if !model.search.is_empty() {
        let clear = actions.clone();
        row = row.child(icon_button(
            IconName::Close,
            theme.palette().foreground,
            "清除搜索",
            actions,
            move |_tree, _id| clear.push(Action::ClearSearch),
        ));
    }
    row
}

/// The rename / tags edit bar: a text field, the caret, and save / cancel.
pub(super) fn edit_bar(theme: &'static dyn Theme, edit: &EditState, actions: &Actions) -> Column {
    let label = match edit.kind {
        EditKind::Name => "改名",
        EditKind::Tags => "标签（用逗号分隔）",
        EditKind::Search => "搜索",
        EditKind::CatalogSearch => "搜索核心",
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
                .child(text_field(theme, actions, "").grow(1.0))
                .child(
                    Button::primary("保存", theme)
                        .mini()
                        .on_click(move |_tree, _id| save.push(Action::CommitEdit)),
                )
                .child(
                    Button::ghost("取消", theme)
                        .mini()
                        .on_click(move |_tree, _id| cancel.push(Action::CancelEdit)),
                ),
        )
}

/// The library as a fixed-column grid of cover cards, mounted a window at a
/// time. The whole thing is the content of the library page's [`ScrollView`].
///
/// Only the rows the viewport covers are built (plus one row of slack), with
/// spacers above and below standing in for the rest, so the scrollbar and the
/// offset stay correct while the cost of a scroll step depends on the viewport
/// rather than on how many games the library holds.
pub(super) fn library_grid(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
) -> Column {
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

/// The library grid's mounted row range for the model's scroll state.
pub(super) fn library_grid_window(model: &ViewModel) -> (usize, usize) {
    window_for(
        model.games.len(),
        model.grid_columns,
        model.grid_offset,
        grid_viewport(model),
        CARD_HEIGHT + space::SM,
    )
}

/// One library cell: the cover (with the console badge and the card controls
/// over it), then the name under it. Clicking the cover or name starts the
/// game; the controls are their own buttons, so they never start it (the
/// nearest callback wins).
pub(super) fn game_card(
    theme: &'static dyn Theme,
    model: &ViewModel,
    game: &GameRow,
    index: usize,
    actions: &Actions,
) -> Column {
    let click = actions.clone();
    let menu = actions.clone();
    let playing = model.selected == Some(index);
    let mut card = Column::new()
        .gap(space::XXS)
        .padding(Edges::all(space::XXS))
        // A fixed height keeps the rows uniform for the virtualized grid, so
        // the tags line below can be omitted when empty.
        .min_size(0.0, CARD_HEIGHT)
        .dynamic_background(move |state| {
            let fill = if playing {
                theme.palette().selection
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill)
        })
        .cursor(Cursor::Pointer)
        .on_click(move |_tree, _id| click.push(Action::CardActivate(index)))
        .on_secondary_click(move |_tree, _id, position| {
            menu.push(Action::GameContextMenu { index, position })
        })
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
        );
    // Only draw the tags line when there are tags: an empty `Text` is a
    // zero-glyph draw command (and the card height is fixed above anyway).
    if !game.tags.is_empty() {
        card = card.child(
            Text::caption(tags_label(game), theme)
                .tone(Tone::Muted)
                .max_lines(1)
                .ellipsis(true),
        );
    } else {
        card = card.child(Text::caption("", theme).max_lines(1).ellipsis(true))
    }
    // The card's bottom row: a hint on the left, the one-click play button on
    // the right. Both sit in the card's own flex column (no positioning).
    card = card.child(Divider::horizontal(theme)).child(
        Row::new()
            .align(Align::Center)
            .child(play_button(theme, index, actions).grow(1.0)),
    );
    card
}

/// A card's tags as one line: the first few `#words`, then a `+N` count. A
/// single ellipsized line, because the cells are narrow and a wrapping row of
/// chips would make every card in the row taller.
pub(super) fn tags_label(game: &GameRow) -> String {
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
pub(super) fn cover(
    theme: &'static dyn Theme,
    game: &GameRow,
    index: usize,
    actions: &Actions,
) -> Column {
    let color = cover_color(&game.path);
    let mut cover = Column::new()
        .gap(space::XXS)
        // No outer padding: the controls sit flush in the top-right corner.
        .padding(Edges::ZERO)
        .min_size(0.0, PLACEHOLDER_HEIGHT)
        .surface(SurfaceStyle::new(color).radius(radius::SM))
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
                .background(media::SCRIM)
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
                            .color(cover_ink(color))
                            .max_lines(3)
                            .ellipsis(true),
                    ),
            );
        }
    }
    cover
}

/// The play button on a card's bottom-right: a single click starts the game.
/// The card itself plays on a double click, so this is the deliberate
/// one-click path; the hint beside it says as much.
fn play_button(theme: &'static dyn Theme, index: usize, actions: &Actions) -> impl Component {
    let click = actions.clone();
    Button::new("", theme)
        .cursor(Cursor::Pointer)
        .on_click(move |_tree, _id| click.push(Action::Play(index)))
        .child(Text::caption("立即游玩", theme))
}

/// The console badge, top-left on the cover: the short name the console is
/// known by ("NES", "GBA", "GB"). Compact and unpadded, like the controls.
pub(super) fn system_badge(theme: &'static dyn Theme, game: &GameRow) -> Flex {
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
        .surface(SurfaceStyle::new(media::BADGE).radius(radius::SM))
        .child(Text::caption(game.system.short(), theme).color(media::ON_MEDIA_MUTED))
}

/// The card's controls, top-right on the cover: the screenshot count (when
/// there are any), the pin, then delete. The pin is yellow when the game is
/// pinned; delete destroys the ROM.
pub(super) fn card_controls(
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
    let ink = media::ON_MEDIA_MUTED;
    let pin_ink = if game.pinned {
        theme.palette().warning
    } else {
        ink
    };
    row = row.child(icon_button(IconName::Pencil, ink, "改名", actions, {
        let actions = actions.clone();
        move |_tree, _id| actions.push(Action::StartRename(game_id))
    }));
    row = row.child(icon_button(IconName::Tag, ink, "标签", actions, {
        let actions = actions.clone();
        move |_tree, _id| actions.push(Action::StartTagEdit(game_id))
    }));
    let pin_tip = if game.pinned {
        "取消置顶"
    } else {
        "置顶"
    };
    row.child(icon_button(IconName::Pin, pin_ink, pin_tip, actions, {
        let actions = actions.clone();
        move |_tree, _id| actions.push(Action::TogglePin(index))
    }))
    .child(icon_button(IconName::Trash, ink, "删除", actions, {
        let actions = actions.clone();
        move |_tree, _id| actions.push(Action::RequestDelete(Confirm::DeleteGame(game_id)))
    }))
}

/// The card's screenshot count: a camera and the number, opening the
/// screenshots section for this game. Only shown when the game has any.
pub(super) fn screenshot_entry(
    theme: &'static dyn Theme,
    game: &GameRow,
    actions: &Actions,
) -> impl Component {
    let node = NodeRef::new();
    actions.tip(&node, "截图");
    let click = actions.clone();
    let game_id = game.id;
    let ink = media::ON_MEDIA_MUTED;
    compact_button(move |_tree, _id| click.push(Action::ShowScreenshots(game_id)))
        .gap(2.0)
        .child(SvgIcon::new(IconName::Camera, ink, CARD_ICON))
        .child(Text::caption(game.screenshots.to_string(), theme).color(ink))
        .ref_(&node)
}

/// A card's meta line: its size, and either the play count and time or the
/// fact that it has never been run.
pub(super) fn meta_label(game: &GameRow) -> String {
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
pub(super) fn format_size(bytes: u64) -> String {
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
pub(super) fn format_duration(seconds: i64) -> String {
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
pub(super) fn cover_color(path: &str) -> Color {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hsv((hash % 360) as f32, 0.45, 0.45)
}

/// Ink that stays legible on a cover color: dark on a light hue, light on a
/// dark one.
pub(super) fn cover_ink(color: Color) -> Color {
    let [r, g, b, _] = color.to_rgba8();
    let luminance = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
    if luminance > 0.55 {
        media::ON_MEDIA_DARK
    } else {
        media::ON_MEDIA
    }
}

/// HSV (h in degrees) to an RGB [`Color`]. Only used for cover hues.
pub(super) fn hsv(hue: f32, saturation: f32, value: f32) -> Color {
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
