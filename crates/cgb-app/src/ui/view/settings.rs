//! The settings page. The middle column lists the functional groups and the
//! right column shows the selected group's cards, so the page never becomes one
//! endless scroll. The groups are: the game library, the appearance, the
//! picture, the console cores, the inputs and the core downloader.

use igui::igui_components::{
    Button, Card, Column, Component, NodeRef, Row, ScrollView, Select, Text,
};
use igui::igui_core::{Color, Edges};
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SizeBasis, SurfaceStyle};

use crate::ui::icons::{Icon as SvgIcon, IconName};
use crate::ui::model::{Action, EditTarget, MsaaKind, SettingsGroup, ShaderKind, ViewModel};
use crate::ui::theme::ThemeChoice;

use super::components::{chip, chip_group, text_field};
use super::Page;
use super::ViewBridge;

/// The widest the settings detail column grows, so its cards do not stretch
/// across a wide window; it shrinks to fit a narrow one.
const DETAIL_MAX_WIDTH: f32 = 720.0;

/// The "download a core" card. The catalog is a local copy of the libretro
/// buildbot's list (the built-in snapshot, or the cache `刷新下载源` writes);
/// downloading runs on a background thread and reports through
/// `catalog_status` / `catalog_progress`.
fn catalog_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
    let mut search = Row::new().align(Align::Center).gap(space::XS);
    if let Some(edit) = &model.editing {
        if *edit == EditTarget::CatalogSearch {
            let done = actions.clone();
            search = search
                .child(text_field(theme, actions, "核心名称 / 机种…").grow(1.0))
                .child(
                    Button::primary("完成", theme)
                        .mini()
                        .on_click(move |_tree, _id| done.push(Action::CommitEdit)),
                );
        }
    } else {
        let start = actions.clone();
        let label = if model.catalog_query.is_empty() {
            "搜索可下载的核心…".to_string()
        } else {
            format!("搜索：{}", model.catalog_query)
        };
        search = search.child(
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
                .on_click(move |_tree, _id| start.push(Action::StartCatalogSearch)),
        );
    }
    let refresh = actions.clone();
    search = search.child(
        Button::secondary("刷新下载源", theme)
            .mini()
            .on_click(move |_tree, _id| refresh.push(Action::RefreshCatalog)),
    );

    let status = if let Some(name) = &model.catalog_downloading {
        match model.catalog_progress {
            Some(progress) => format!("正在下载 {name}… {:.0}%", progress * 100.0),
            None => format!("正在下载 {name}…"),
        }
    } else {
        model.catalog_status.clone()
    };

    let mut list = Column::new().gap(space::XS);
    if !status.is_empty() {
        list = list.child(Text::caption(status, theme).tone(Tone::Muted));
    }
    if model.catalog.is_empty() {
        list = list.child(
            Text::small(
                format!(
                    "输入名称搜索可下载的核心（共 {} 个）。",
                    model.catalog_total
                ),
                theme,
            )
            .tone(Tone::Muted),
        );
    } else {
        let mut any_unsupported = false;
        for (index, entry) in model.catalog.iter().enumerate() {
            let mut item = Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .child(
                    Text::small(entry.display_name.as_str(), theme)
                        .grow(1.0)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(Text::caption(entry.system_key.as_str(), theme).tone(Tone::Muted));
            if !entry.supported {
                any_unsupported = true;
                item = item.child(Text::caption("暂不支持", theme).tone(Tone::Subtle));
            } else if entry.downloaded {
                item = item.child(Button::ghost("已下载", theme).mini());
            } else {
                let download = actions.clone();
                item = item.child(
                    Button::secondary("下载", theme)
                        .mini()
                        .on_click(move |_tree, _id| download.push(Action::DownloadCore(index))),
                );
            }
            list = list.child(item);
        }
        list = list.child(
            Text::caption(
                format!(
                    "显示 {} / 共 {} 个",
                    model.catalog.len(),
                    model.catalog_total
                ),
                theme,
            )
            .tone(Tone::Muted),
        );
        if any_unsupported {
            list = list.child(
                Text::caption(
                    "「暂不支持」= 该机种本应用尚未支持，下载后也无法使用。",
                    theme,
                )
                .tone(Tone::Subtle),
            );
        }
    }

    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(search)
        .child(list)
}

/// The game-library card: the one library folder, and the switch button.
fn library_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
    let current = match model.library_root.as_deref() {
        Some(path) => Text::small(path, theme)
            .grow(1.0)
            .max_lines(1)
            .ellipsis(true),
        None => Text::small("还没有游戏库，选一个文件夹作为游戏库。", theme).tone(Tone::Muted),
    };
    let switch = actions.clone();
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(current)
        .child(
            Button::secondary("切换游戏库…", theme)
                .on_click(move |_tree, _id| switch.push(Action::SwitchLibrary)),
        )
}

/// The appearance card: the theme family and light / dark.
fn appearance_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
    let mut theme_chips = chip_group();
    for choice in ThemeChoice::ALL {
        let pick = actions.clone();
        theme_chips = theme_chips.child(
            chip(theme, choice.label(), model.theme_choice == choice)
                .on_click(move |_tree, _id| pick.push(Action::SetThemeChoice(choice))),
        );
    }
    let mut look_chips = chip_group();
    for light in [false, true] {
        let pick = actions.clone();
        let label = if light { "浅色" } else { "深色" };
        look_chips = look_chips.child(
            chip(theme, label, model.light == light)
                .on_click(move |_tree, _id| pick.push(Action::SetLight(light))),
        );
    }
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::caption("主题", theme).tone(Tone::Muted))
        .child(theme_chips)
        .child(Text::caption("明暗", theme).tone(Tone::Muted))
        .child(look_chips)
}

/// The game-picture post-process card.
fn shader_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
    let mut shader_chips = Row::new().gap(space::XS);
    for kind in ShaderKind::ALL {
        let actions = actions.clone();
        shader_chips = shader_chips.child(
            chip(theme, kind.label(), model.shader == kind)
                .on_click(move |_tree, _id| actions.push(Action::SetShader(kind))),
        );
    }
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::subheading("画面效果", theme))
        .child(shader_chips)
        .child(Text::caption("对游戏画面做后处理，不影响界面。", theme).tone(Tone::Subtle))
}

/// The geometry anti-aliasing card. Off / 2x / 4x force a sample count; Auto
/// uses 4x while idle and drops it while a game runs (the live image dominates).
fn msaa_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
    let mut msaa_chips = Row::new().gap(space::XS);
    for mode in MsaaKind::ALL {
        let actions = actions.clone();
        msaa_chips = msaa_chips.child(
            chip(theme, mode.label(), model.msaa == mode)
                .on_click(move |_tree, _id| actions.push(Action::SetMsaa(mode))),
        );
    }
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
        )
}

/// One core pick per console. The selected one is the core that would run the
/// console now (the saved pick, or the manifest default).
fn cores_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
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
            .on_open(move |_tree, _id| {
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
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::subheading("核心选择", theme))
        .child(cores)
}

/// The running core's own options, when a game has been loaded.
fn core_options_card(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Card {
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
                        .on_click(move |_tree, _id| {
                            previous.push(Action::CycleCoreOption(index, -1))
                        }),
                )
                .child(Text::caption(option.value.as_str(), theme).tone(Tone::Muted))
                .child(
                    Button::ghost("›", theme)
                        .mini()
                        .on_click(move |_tree, _id| next.push(Action::CycleCoreOption(index, 1))),
                ),
        );
    }
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::subheading("核心选项", theme))
        .child(options)
}

/// Keyboard bindings, read-only for now.
fn bindings_card(theme: &'static dyn Theme, model: &ViewModel) -> Card {
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
        "键盘绑定".to_string()
    } else {
        format!("键盘绑定（{}）", model.bindings_system)
    };
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::subheading(title, theme))
        .child(bindings)
        .child(
            Text::caption("键盘按机种分别保存；手柄由 macOS 自动映射。", theme).tone(Tone::Subtle),
        )
}

/// The core's own input descriptors, when a game has been loaded: mGBA's
/// shoulder buttons, an arcade stick's buttons, and so on.
fn core_inputs_card(theme: &'static dyn Theme, model: &ViewModel) -> Card {
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
    Card::new(theme)
        .gap(space::SM)
        .padding(Edges::all(space::SM))
        .child(Text::subheading("核心输入", theme))
        .child(inputs)
}

/// One entry in the group nav: the label, a muted note and the selected fill.
fn group_item(
    theme: &'static dyn Theme,
    group: SettingsGroup,
    active: SettingsGroup,
    actions: &ViewBridge,
) -> Column {
    let actions = actions.clone();
    let selected = group == active;
    let mut row = Row::new()
        .align(Align::Center)
        .gap(space::XS)
        .child(Text::small(group.label(), theme).grow(1.0));
    if selected {
        row = row.child(SvgIcon::new(
            IconName::ChevronRight,
            theme.palette().muted,
            12.0,
        ));
    }
    Column::new()
        .gap(space::XXS)
        .padding(Edges::new(space::SM, space::XS, space::SM, space::XS))
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
        .on_click(move |_tree, _id| actions.push(Action::ShowSettingsGroup(group)))
        .child(row)
        .child(Text::caption(group.description(), theme).tone(Tone::Muted))
}

/// The middle column: the title and the group nav. The selected group's cards
/// are mounted in the right column by [`settings_detail`].
pub(super) fn settings_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Page {
    let mut column = Column::new()
        .gap(space::MD)
        .padding(Edges::all(space::MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title("设置", theme));
    let mut nav = Column::new()
        .gap(space::XS)
        .mouse_filter(MouseFilter::Ignore);
    for group in SettingsGroup::ALL {
        nav = nav.child(group_item(theme, group, model.settings_group, actions));
    }
    column = column.child(nav);
    Page {
        tree: column,
        scroll: None,
    }
}

/// The right column's settings detail: the title of the selected group and its
/// cards, centered and capped so they do not stretch across a wide window. It
/// owns the settings page's scroll.
pub(super) fn settings_detail(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Page {
    let group = model.settings_group;
    let mut body = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);
    body = match group {
        SettingsGroup::Library => body.child(library_card(theme, model, actions)),
        SettingsGroup::Appearance => body.child(appearance_card(theme, model, actions)),
        SettingsGroup::Display => body
            .child(shader_card(theme, model, actions))
            .child(msaa_card(theme, model, actions)),
        SettingsGroup::Cores => {
            let body = body.child(cores_card(theme, model, actions));
            if model.core_options.is_empty() {
                body
            } else {
                body.child(core_options_card(theme, model, actions))
            }
        }
        SettingsGroup::Input => {
            let body = body.child(bindings_card(theme, model));
            if model.core_inputs.is_empty() {
                body
            } else {
                body.child(core_inputs_card(theme, model))
            }
        }
        SettingsGroup::Download => body.child(catalog_card(theme, model, actions)),
    };

    // Center the capped column; it shrinks below the cap on a narrow window.
    let centered = Row::new()
        .justify(Justify::Center)
        .mouse_filter(MouseFilter::Ignore)
        .child(body.basis(SizeBasis::Px(DETAIL_MAX_WIDTH)).shrink(1.0));

    // Only the bodies scroll; the title stays put.
    let view = ScrollView::new(theme)
        .grow(1.0)
        .scrollbar(false)
        .child(centered);
    let scroll = Some(view.state());

    let column = Column::new()
        .gap(space::MD)
        .padding(Edges::all(space::MD))
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title(group.label(), theme))
        .child(view);
    Page {
        tree: column,
        scroll,
    }
}
