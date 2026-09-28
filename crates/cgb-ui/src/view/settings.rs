//! The settings page: the game library, the per-console core pick, the picture
//! effect, the running core's options and inputs, and the keyboard bindings.

use igui::igui_components::{
    Button, Card, Column, Component, NodeRef, Row, ScrollView, ScrollViewState, Select, Text,
};
use igui::igui_core::Edges;
use igui::igui_theme::{space, Theme, Tone};
use igui::igui_ui::{Align, MouseFilter};

use crate::model::{Action, MsaaKind, ShaderKind, ViewModel};
use crate::theme::ThemeChoice;

use super::components::{chip, chip_group};
use super::Actions;

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

    // Appearance: the theme family and light / dark.
    let mut theme_chips = chip_group();
    for choice in ThemeChoice::ALL {
        let pick = actions.clone();
        theme_chips = theme_chips.child(
            chip(theme, choice.label(), model.theme_choice == choice)
                .on_click(move || pick.push(Action::SetThemeChoice(choice))),
        );
    }
    let mut look_chips = chip_group();
    for light in [false, true] {
        let pick = actions.clone();
        let label = if light { "浅色" } else { "深色" };
        look_chips = look_chips.child(
            chip(theme, label, model.light == light)
                .on_click(move || pick.push(Action::SetLight(light))),
        );
    }
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("外观", theme))
            .child(Text::caption("主题", theme).tone(Tone::Muted))
            .child(theme_chips)
            .child(Text::caption("明暗", theme).tone(Tone::Muted))
            .child(look_chips),
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

    // Geometry anti-aliasing. Off / 2x / 4x force a sample count; Auto uses
    // 4x while idle and drops it while a game runs (the live image dominates).
    let mut msaa_chips = Row::new().gap(space::XS);
    for mode in MsaaKind::ALL {
        let actions = actions.clone();
        msaa_chips = msaa_chips.child(
            chip(theme, mode.label(), model.msaa == mode)
                .on_click(move || actions.push(Action::SetMsaa(mode))),
        );
    }
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("抗锯齿", theme))
            .child(msaa_chips)
            .child(
                Text::caption(
                    "几何边缘的多重采样；「自动」在游戏运行时关闭以省 GPU。",
                    theme,
                )
                .tone(Tone::Subtle),
            ),
    );

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
