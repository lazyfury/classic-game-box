//! The console column on the right: the live picture and its controls, the
//! immersive fullscreen view, and the screenshot preview it swaps in.

use igui::igui_components::{Badge, Button, Column, Component, Flex, NodeRef, Row, Text};
use igui::igui_core::{Color, Edges};
use igui::igui_theme::radius::MD;
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle};

use crate::ui::frame::FrameImage;
use crate::ui::icons::{Icon as SvgIcon, IconName};
use crate::ui::model::{Action, Confirm, ScreenshotRow, ViewModel};

use super::format::format_when;
use super::ViewBridge;

/// The console column, on the right and always mounted. While a screenshot is
/// being previewed it shows the picture instead of the console.
pub(super) fn play_column(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
    info_ref: &NodeRef,
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
        .selected_game()
        .map(|game| game.name.clone())
        .or_else(|| (!model.core_name.is_empty()).then(|| model.core_name.clone()))
        .unwrap_or_else(|| "没有选中游戏".to_string());
    let mut title_row = Row::new().align(Align::Center).gap(space::SM).child(
        Text::subheading(title, theme)
            .grow(1.0)
            .max_lines(1)
            .ellipsis(true),
    );
    if !model.info.is_empty() {
        title_row = title_row.child(
            Text::caption(model.info.clone(), theme)
                .tone(Tone::Muted)
                .max_lines(1)
                .ref_(info_ref),
        );
    }
    column = column.child(title_row);

    // The framebuffer itself: it grows into the remaining space and centres a
    // letterboxed picture inside whatever rectangle it gets.
    match &model.frame {
        Some(frame) => {
            let mut image = FrameImage::new(frame.texture, frame.width, frame.height).grow(1.0);
            if model.paused {
                image = image.child(paused_overlay(theme));
            }
            column = column.child(image);
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
    controls = controls.child(
        Button::primary(pause, theme).on_click(move |_tree, _id| toggle.push(Action::TogglePause)),
    );
    let reset = actions.clone();
    controls = controls.child(
        Button::secondary("复位", theme).on_click(move |_tree, _id| reset.push(Action::Reset)),
    );
    let rewind = actions.clone();
    controls = controls.child(
        Button::ghost("倒带", theme).on_click(move |_tree, _id| rewind.push(Action::Rewind)),
    );
    let save = actions.clone();
    controls = controls.child(
        Button::secondary("快速存档", theme)
            .on_click(move |_tree, _id| save.push(Action::QuickSave)),
    );
    let load = actions.clone();
    controls = controls.child(
        Button::secondary("快速读档", theme)
            .on_click(move |_tree, _id| load.push(Action::LoadQuick(0))),
    );
    let shot = actions.clone();
    controls = controls.child(
        Button::ghost("截图", theme).on_click(move |_tree, _id| shot.push(Action::Screenshot)),
    );
    let cover = actions.clone();
    controls = controls.child(
        Button::ghost("设为封面", theme)
            .on_click(move |_tree, _id| cover.push(Action::ScreenshotCover)),
    );
    let assign = actions.clone();
    controls = controls.child(
        Button::ghost("输入分配", theme)
            .on_click(move |_tree, _id| assign.push(Action::OpenInputAssign)),
    );
    let full = actions.clone();
    let full_label = if model.fullscreen {
        "退出全屏"
    } else {
        "全屏"
    };
    controls = controls.child(
        Button::secondary(full_label, theme)
            .on_click(move |_tree, _id| full.push(Action::ToggleFullscreen)),
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
    actions: &ViewBridge,
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
            Text::subheading(shot.game_name.as_str(), theme)
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
                    .on_click(move |_tree, _id| previous.push(Action::StepPreview(-1)))
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
                    .on_click(move |_tree, _id| next.push(Action::StepPreview(1)))
                    .ref_(&next_node),
            ),
    );

    let mut controls = Row::new().gap(space::SM);
    if !shot.is_cover {
        let set = actions.clone();
        let id = shot.id;
        controls = controls.child(
            Button::secondary("设为封面", theme)
                .on_click(move |_tree, _id| set.push(Action::SetCover(id))),
        );
    }
    let reveal = actions.clone();
    let reveal_id = shot.id;
    controls = controls.child(
        Button::ghost("在访达中显示", theme)
            .on_click(move |_tree, _id| reveal.push(Action::RevealScreenshot(reveal_id))),
    );
    let remove = actions.clone();
    let remove_id = shot.id;
    controls = controls.child(
        Button::destructive("删除", theme).on_click(move |_tree, _id| {
            remove.push(Action::RequestDelete(Confirm::DeleteScreenshot(remove_id)))
        }),
    );
    let close = actions.clone();
    controls = controls.child(
        Button::secondary("关闭", theme)
            .on_click(move |_tree, _id| close.push(Action::ClosePreview)),
    );
    column.child(controls)
}

/// A large centered "暂停" badge. It is mounted as a child of the frame image,
/// whose leaf container resolves the fill anchors over the picture rectangle
/// (no separate overlay layer needed).
fn paused_overlay(theme: &'static dyn Theme) -> Flex {
    Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        // Fill the frame's rectangle, then centre the badge inside it.
        .anchors(Edges::new(0.0, 0.0, 1.0, 1.0))
        .offsets(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Flex::row()
                .align(Align::Center)
                .justify(Justify::Center)
                .padding(Edges::new(space::MD, space::SM, space::MD, space::SM))
                .surface(SurfaceStyle::new(Color::new(0.0, 0.0, 0.0, 0.55)).radius(radius::MD))
                .mouse_filter(MouseFilter::Ignore)
                .child(Text::heading("暂停", theme).color(Color::WHITE)),
        )
}
