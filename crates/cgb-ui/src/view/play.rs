//! The console column on the right: the live picture and its controls, the
//! immersive fullscreen view, and the screenshot preview it swaps in.

use igui::igui_components::{Badge, Button, Column, Component, Divider, Flex, NodeRef, Row, Text};
use igui::igui_core::Edges;
use igui::igui_theme::radius::MD;
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle};

use crate::frame::FrameImage;
use crate::icons::{Icon as SvgIcon, IconName};
use crate::model::{Action, Confirm, ScreenshotRow, ViewModel};

use super::components::format_when;
use super::Actions;

/// The immersive play view: the game picture fills everything below a slim
/// overlay bar with pause / save / load / screenshot / exit controls.
///
/// The rest of the shell (header, rail, library, status line) is not mounted,
/// which is what keeps a playing frame cheap: the visible grid alone added
/// thousands of draw commands to every frame.
pub(super) fn fullscreen_play(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
) -> Column {
    let mut column = Column::new()
        .gap(0.0)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(fullscreen_bar(theme, model, actions));
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
                    .surface(SurfaceStyle::new(theme.palette().surface))
                    .child(Text::small("没有画面：还没有载入游戏。", theme).tone(Tone::Muted)),
            );
        }
    }
    column
}

/// The thin bar across the top of the immersive view. It carries the game name,
/// any status message, and the controls that matter in fullscreen; everything
/// else is one Escape (or F11) away.
pub(super) fn fullscreen_bar(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
) -> Column {
    let title = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.name.clone())
        .or_else(|| (!model.core_name.is_empty()).then(|| model.core_name.clone()))
        .unwrap_or_else(|| "没有选中游戏".to_string());
    let pause = if model.paused { "继续" } else { "暂停" };

    let mut row = Row::new()
        .align(Align::Center)
        .gap(space::SM)
        .padding(Edges::new(space::MD, space::XS, space::MD, space::XS))
        .surface(SurfaceStyle::new(theme.palette().surface));
    row = row.child(
        Text::small(title, theme)
            .grow(1.0)
            .max_lines(1)
            .ellipsis(true),
    );
    if !model.status.is_empty() {
        row = row.child(
            Text::caption(model.status.clone(), theme)
                .tone(Tone::Muted)
                .max_lines(1)
                .ellipsis(true),
        );
    }

    let toggle = actions.clone();
    row = row
        .child(Button::secondary(pause, theme).on_click(move || toggle.push(Action::TogglePause)));
    let save = actions.clone();
    row = row.child(Button::ghost("存档", theme).on_click(move || save.push(Action::SaveState(0))));
    let load = actions.clone();
    row = row.child(Button::ghost("读档", theme).on_click(move || load.push(Action::LoadState(0))));
    let shot = actions.clone();
    row = row.child(Button::ghost("截图", theme).on_click(move || shot.push(Action::Screenshot)));
    let exit = actions.clone();
    row = row.child(
        Button::secondary("退出全屏", theme).on_click(move || exit.push(Action::ToggleFullscreen)),
    );

    Column::new()
        .gap(0.0)
        .child(row)
        .child(Divider::horizontal(theme))
}

/// The console column, on the right and always mounted. While a screenshot is
/// being previewed it shows the picture instead of the console.
pub(super) fn play_column(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
) -> Column {
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
    let full = actions.clone();
    controls = controls.child(
        Button::secondary("全屏", theme).on_click(move || full.push(Action::ToggleFullscreen)),
    );
    column = column.child(controls);

    if !model.core_name.is_empty() {
        column = column
            .child(Text::caption(format!("核心：{}", model.core_name), theme).tone(Tone::Muted));
    }
    column
}

/// The play column while a screenshot is previewed: the picture large, with
/// previous/next and the same controls as the grid.
pub(super) fn preview_column(
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
    let prev_node = NodeRef::new();
    let next_node = NodeRef::new();
    actions.tip(&prev_node, "上一张");
    actions.tip(&next_node, "下一张");
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
                    .on_click(move || previous.push(Action::StepPreview(-1)))
                    .ref_(&prev_node),
            )
            .child(Text::caption(format_when(shot.created_at), theme).tone(Tone::Subtle))
            .child(
                Button::ghost("", theme)
                    .child(SvgIcon::new(
                        IconName::ChevronRight,
                        theme.palette().foreground,
                        14.0,
                    ))
                    .on_click(move || next.push(Action::StepPreview(1)))
                    .ref_(&next_node),
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
