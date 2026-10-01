//! The cheats section: the running game's cheat list, with toggles and a `.cht`
//! import.

use igui::igui_components::{Button, Column, Component, EmptyState, Row, ScrollView, Text};
use igui::igui_core::Edges;
use igui::igui_theme::radius::MD;
use igui::igui_theme::{space, Theme, Tone};
use igui::igui_ui::{Align, MouseFilter};

use crate::ui::model::{Action, CheatRow, ViewModel};

use super::Page;
use super::ViewBridge;

/// The cheats section: the running game's cheat list, with toggles and a
/// `.cht` import.
pub(super) fn cheats_page(
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
        .child(Text::heading("金手指", theme));
    let subtitle = match &game {
        Some(name) => format!("{name} · {}", model.core_name),
        None => "没有正在运行的游戏".to_string(),
    };
    column = column.child(Text::caption(subtitle, theme).tone(Tone::Muted));

    if !model.has_session {
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
        scroll = Some(view.state());
        column = column.child(view);
    }

    let import = actions.clone();
    column = column.child(
        Button::secondary("导入 .cht…", theme)
            .on_click(move |_tree, _id| import.push(Action::ImportCheats)),
    );
    column = column.child(
        Text::caption(
            "码的语法由核心决定：Mesen 认 Game Genie / PAR / AAAA:VV，mGBA 认 GBA 码；FBNeo 不支持金手指。",
            theme,
        )
        .tone(Tone::Subtle)
        .max_lines(2),
    );
    Page {
        tree: column,
        scroll,
    }
}

/// One cheat: its description and code, and an on/off toggle.
pub(super) fn cheat_row(
    theme: &'static dyn Theme,
    index: usize,
    cheat: &CheatRow,
    actions: &ViewBridge,
) -> Row {
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
                .on_click(move |_tree, _id| toggle.push(Action::ToggleCheat(index))),
        )
}
