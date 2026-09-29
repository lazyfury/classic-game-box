//! The saves section: the running game's save-state slots for its core.

use igui::igui_components::{
    Button, Column, Component, EmptyState, Flex, Row, ScrollView, ScrollViewState, Text,
};
use igui::igui_core::Edges;
use igui::igui_render::Paint;
use igui::igui_theme::radius::MD;
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle};

use crate::frame::cover_fit;
use crate::model::{Action, SaveSlotRow, ViewModel};

use super::components::format_when;
use super::Actions;

/// The saves section: the running game's save-state slots for its core.
pub(super) fn saves_page(
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
pub(super) fn save_slot_row(
    theme: &'static dyn Theme,
    row: &SaveSlotRow,
    actions: &Actions,
) -> Row {
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
                .on_click(move |_tree, _id| save.push(Action::SaveToSlot(slot))),
        )
        .child(
            Button::ghost("读", theme)
                .mini()
                .on_click(move |_tree, _id| load.push(Action::LoadFromSlot(slot))),
        )
        .child(
            Button::ghost("删", theme)
                .mini()
                .on_click(move |_tree, _id| delete.push(Action::DeleteSlot(slot))),
        )
}

/// A save slot's thumbnail, cropped to fill a small cell.
pub(super) fn save_thumb(theme: &'static dyn Theme, row: &SaveSlotRow) -> Flex {
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
