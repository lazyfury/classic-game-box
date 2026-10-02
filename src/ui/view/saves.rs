//! The saves section: the running game's rolling quick saves and fixed manual
//! slots for its core.

use igui::igui_components::{Button, Column, Component, EmptyState, Flex, Row, ScrollView, Text};
use igui::igui_core::Edges;
use igui::igui_render::Paint;
use igui::igui_theme::radius::MD;
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle};

use crate::ui::frame::crop_fit;
use crate::ui::model::{manual_slot_label, quick_slot_label, Action, SaveSlotRow, ViewModel};

use super::format::format_when;
use super::Page;
use super::ViewBridge;

/// Which save stack a row belongs to, so its buttons build the right action.
#[derive(Clone, Copy)]
enum SlotKind {
    /// A rolling quick save, by rank (`0` is the newest).
    Quick(u8),
    /// A fixed manual slot (`1`..=9).
    Manual(u8),
}

impl SlotKind {
    fn label(self) -> String {
        match self {
            SlotKind::Quick(rank) => quick_slot_label(rank),
            SlotKind::Manual(slot) => manual_slot_label(slot),
        }
    }

    /// The "save" action: quick save always lands in the newest slot, whatever
    /// row it was pressed from.
    fn save(self) -> Action {
        match self {
            SlotKind::Quick(_) => Action::QuickSave,
            SlotKind::Manual(slot) => Action::SaveToSlot(slot),
        }
    }

    fn load(self) -> Action {
        match self {
            SlotKind::Quick(rank) => Action::LoadQuick(rank),
            SlotKind::Manual(slot) => Action::LoadFromSlot(slot),
        }
    }

    fn delete(self) -> Action {
        match self {
            SlotKind::Quick(rank) => Action::DeleteQuick(rank),
            SlotKind::Manual(slot) => Action::DeleteSlot(slot),
        }
    }
}

/// The saves section: the running game's save-state slots for its core.
pub(super) fn saves_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Page {
    let mut scroll = None;
    let game = model.selected_game().map(|game| game.name.clone());
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

    if !model.has_session {
        column = column.child(
            EmptyState::new("没有正在运行的游戏", theme).description("开始一个游戏后才能存读档。"),
        );
    } else if !model.save_states_supported {
        column = column.child(
            EmptyState::new("该核心不支持即时存档", theme)
                .description("这个 core 没有实现 retro_serialize。"),
        );
    } else {
        let mut list = Column::new().gap(space::XS);
        list = list
            .child(Text::small("快速存档", theme).tone(Tone::Muted))
            .child(Text::caption("最近 3 次，新的在前 · F5 存 / F6 读", theme).tone(Tone::Subtle));
        for row in &model.quick_saves {
            list = list.child(save_slot_row(
                theme,
                row,
                actions,
                SlotKind::Quick(row.slot),
            ));
        }
        list = list
            .child(Text::small("存档槽", theme).tone(Tone::Muted))
            .child(Text::caption("固定槽位 · F1–F3 存 / Shift+F1–F3 读", theme).tone(Tone::Subtle));
        for row in &model.save_states {
            list = list.child(save_slot_row(
                theme,
                row,
                actions,
                SlotKind::Manual(row.slot),
            ));
        }
        let view = ScrollView::new(theme)
            .scrollbar(false)
            .grow(1.0)
            .child(list);
        scroll = Some(view.state());
        column = column.child(view);
    }
    Page {
        tree: column,
        scroll,
    }
}

/// One save slot: its thumbnail, name, time, and the 存 / 读 / 删 buttons.
fn save_slot_row(
    theme: &'static dyn Theme,
    row: &SaveSlotRow,
    actions: &ViewBridge,
    kind: SlotKind,
) -> Row {
    let save = actions.clone();
    let load = actions.clone();
    let delete = actions.clone();
    let time = if row.exists {
        format_when(row.modified_ms)
    } else {
        "空".to_string()
    };
    Row::new()
        .align(Align::Center)
        .gap(space::XS)
        .child(save_thumb(theme, row))
        .child(Text::small(kind.label(), theme).grow(1.0))
        .child(Text::caption(time, theme).tone(Tone::Subtle))
        .child(
            Button::ghost("存", theme)
                .mini()
                .on_click(move |_tree, _id| save.push(kind.save())),
        )
        .child(
            Button::ghost("读", theme)
                .mini()
                .on_click(move |_tree, _id| load.push(kind.load())),
        )
        .child(
            Button::ghost("删", theme)
                .mini()
                .on_click(move |_tree, _id| delete.push(kind.delete())),
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
            let destination = crop_fit((handle.width, handle.height), rect);
            ctx.draw_image(handle.texture, destination, None, Paint::default());
        });
    }
    thumb
}
