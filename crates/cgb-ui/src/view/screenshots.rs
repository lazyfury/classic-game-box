//! The screenshots section: the current game's screenshots, newest first, in a
//! virtualized grid, with preview / cover / delete controls.

use draw_components::{
    Badge, Button, Column, Component, EmptyState, Flex, Grid, NodeRef, Row, ScrollView,
    ScrollViewState, Text,
};
use draw_core::{Color, Edges};
use draw_render::Paint;
use draw_theme::radius::MD;
use draw_theme::{radius, space, Theme, Tone};
use draw_ui::{Align, Justify, MouseFilter, SurfaceStyle, Track};

use crate::frame::cover_fit;
use crate::icons::{Icon as SvgIcon, IconName};
use crate::model::{Action, Confirm, ScreenshotRow, ViewModel};

use super::components::{format_when, grid_viewport, icon_button, spacer, window_for};
use super::Actions;
use super::{CARD_ICON, CARD_ICON_BUTTON, PLACEHOLDER_HEIGHT, SHOT_HEIGHT};

/// The screenshots grid's mounted row range for the model's scroll state. It
/// follows the same filter as [`screenshots_page`].
pub(super) fn screenshots_grid_window(model: &ViewModel) -> (usize, usize) {
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

pub(super) fn screenshots_page(
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
    let folder_node = NodeRef::new();
    actions.tip(&folder_node, "在访达中显示");
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
                        .on_click(move || open.push(Action::OpenScreenshotsFolder))
                        .ref_(&folder_node),
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
pub(super) fn screenshots_grid(
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
pub(super) fn shot_card(
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
pub(super) fn thumbnail(
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
pub(super) fn shot_controls(
    theme: &'static dyn Theme,
    shot: &ScreenshotRow,
    actions: &Actions,
) -> Row {
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
            "设为封面",
            actions,
            move || set.push(Action::SetCover(id)),
        ));
    }
    let reveal = actions.clone();
    let reveal_id = shot.id;
    row = row.child(icon_button(
        IconName::FolderSearch,
        theme.palette().foreground,
        "在访达中显示",
        actions,
        move || reveal.push(Action::RevealScreenshot(reveal_id)),
    ));
    let remove = actions.clone();
    let remove_id = shot.id;
    row.child(icon_button(
        IconName::Trash,
        theme.palette().foreground,
        "删除",
        actions,
        move || remove.push(Action::RequestDelete(Confirm::DeleteScreenshot(remove_id))),
    ))
}
