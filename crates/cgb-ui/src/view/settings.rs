//! The settings page: the game library, the per-console core pick, the picture
//! effect, the running core's options and inputs, and the keyboard bindings.

use draw_components::{
    Button, Card, Column, Component, NodeRef, Row, ScrollView, ScrollViewState, Select, Text,
};
use draw_core::Edges;
use draw_theme::{space, Theme, Tone};
use draw_ui::{Align, MouseFilter};

use crate::model::{Action, ShaderKind, ViewModel};

use super::Actions;

use super::components::chip;

pub(super) fn settings_page(
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

    // The one game library: ROMs, database, screenshots, saves and cheats all
    // live in this folder. There is only one; switching replaces it.
    let current = match model.library_root.as_deref() {
        Some(path) => Text::small(path, theme)
            .grow(1.0)
            .max_lines(1)
            .ellipsis(true),
        None => Text::small("还没有游戏库，选一个文件夹作为游戏库。", theme).tone(Tone::Muted),
    };
    let switch = actions.clone();
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("游戏库", theme))
            .child(current)
            .child(
                Button::secondary("切换游戏库…", theme)
                    .on_click(move || switch.push(Action::SwitchLibrary)),
            ),
    );

    // One core pick per console. The selected one is the core that would run
    // the console now (the saved pick, or the manifest default).
    let mut cores = Column::new().gap(space::SM);
    let mut any_core = false;
    for system in cgb_systems::SYSTEMS {
        // The current pick names the trigger; the host builds the menu of
        // alternatives and anchors it to this node.
        let system = *system;
        let Some(current) = model
            .cores
            .iter()
            .find(|core| core.system == system && core.selected)
        else {
            continue;
        };
        any_core = true;
        let node = NodeRef::new();
        let anchor = node.clone();
        let open = actions.clone();
        let select = Select::new(theme)
            .value(current.name.clone())
            .min_width(170.0)
            .on_open(move || {
                if let Some(anchor) = anchor.get() {
                    open.push(Action::OpenCoreMenu { system, anchor });
                }
            });
        cores = cores.child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .child(Text::small(system.name(), theme).grow(1.0))
                .child(select.ref_(&node)),
        );
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
    let mut shader_chips = Row::new().gap(space::XS);
    for kind in ShaderKind::ALL {
        let actions = actions.clone();
        shader_chips = shader_chips.child(
            chip(theme, kind.label(), model.shader == kind)
                .on_click(move || actions.push(Action::SetShader(kind))),
        );
    }
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("画面效果", theme))
            .child(shader_chips)
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
