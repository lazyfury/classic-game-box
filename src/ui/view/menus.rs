//! Context menus and the destructive confirmation dialog, built on the overlay
//! layer (`igui_components::Overlays`).
//!
//! These live in the view layer, not the host: the menu items, their labels and
//! the actions they push are view content. [`Ui`](crate::ui::Ui) only owns the
//! overlay layer and asks for `repaint`, so the host stays free of view code.

use igui::igui_components::{Column, Component, Menu, MenuItem, Overlays, Text};
use igui::igui_core::{NodeId, Vec2};
use igui::igui_theme::{space, Theme};

use cgb_libretro::{KeyboardMode, SystemId, SYSTEMS};

use crate::host::GamepadDevice;

use super::components::{chip, chip_group};

use crate::ui::model::{Action, Confirm, CoreRow, GameRow};

use super::ViewBridge;

/// How many ports the assignment panel offers per gamepad.
const ASSIGN_PORTS: usize = 4;

/// Open the input-assignment panel: a centered modal listing the connected
/// gamepads, each with its ports (`1P`..`4P`, the current one ticked) and an
/// unassign chip. A modal, not a drop-down: the play-view button sits near the
/// bottom of the window, where a below-anchored menu would be clipped.
pub fn input_modal(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    devices: Vec<GamepadDevice>,
    keyboard_mode: KeyboardMode,
    actions: &ViewBridge,
) {
    let actions = actions.clone();
    let id = overlays.modal("输入分配", move |tree, node| {
        let mut column = Column::new().gap(space::SM);
        column = column.child(Text::small(
            format!("键盘：{}", keyboard_mode.label()),
            theme,
        ));
        if devices.is_empty() {
            column = column.child(Text::small("没有检测到手柄。连上手柄后重新打开。", theme));
        }
        for (index, device) in devices.iter().enumerate() {
            column = column.child(Text::small(
                format!("手柄 {}：{}", index + 1, device.name),
                theme,
            ));
            let mut ports = chip_group();
            for port in 0..ASSIGN_PORTS {
                let actions = actions.clone();
                let device_id = device.id.clone();
                let label = format!("{}P", port + 1);
                ports = ports.child(chip(theme, &label, device.port == Some(port)).on_click(
                    move |_tree, _id| {
                        actions.push(Action::AssignInput {
                            id: device_id.clone(),
                            port: Some(port),
                        })
                    },
                ));
            }
            let actions = actions.clone();
            let device_id = device.id.clone();
            ports = ports.child(chip(theme, "未分配", device.port.is_none()).on_click(
                move |_tree, _id| {
                    actions.push(Action::AssignInput {
                        id: device_id.clone(),
                        port: None,
                    })
                },
            ));
            column = column.child(ports);
        }
        tree.add_child(node, column);
    });
    overlays.confirm_label(id, "关闭");
    overlays.cancel_label(id, "关闭");
    overlays.on_confirm(id, || {});
    overlays.on_cancel(id, || {});
}

/// Open a game card's context menu at `position` (a right click).
pub fn game_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    game: &GameRow,
    index: usize,
    position: Vec2,
    actions: &ViewBridge,
) {
    let game_id = game.id;
    let pinned = game.pinned;
    let actions = actions.clone();
    overlays.menu_at(position, move |tree, node| {
        let item = |label: &str, action: Action| {
            let actions = actions.clone();
            MenuItem::new(label, theme).on_click(move |_tree, _id| actions.push(action.clone()))
        };
        let menu = Menu::new(theme)
            .item(item("开始游戏", Action::Play(index)))
            .item(item("改名…", Action::StartRename(game_id)))
            .item(item(
                "选择机种…",
                Action::OpenSystemMenu {
                    id: game_id,
                    position,
                },
            ))
            .item(item(
                "选择核心…",
                Action::OpenGameCoreMenu {
                    id: game_id,
                    position,
                },
            ))
            .item(item("标签…", Action::StartTagEdit(game_id)))
            .item(item(
                if pinned { "取消置顶" } else { "置顶" },
                Action::TogglePin(index),
            ))
            .item(item("截图", Action::ShowScreenshots(game_id)))
            .separator()
            .item({
                let actions = actions.clone();
                MenuItem::new("删除…", theme)
                    .destructive()
                    .on_click(move |_tree, _id| {
                        actions.push(Action::RequestDelete(Confirm::DeleteGame(game_id)))
                    })
            });
        tree.add_child(node, menu);
    });
}

/// Open a console's core picker, anchored below its `Select` trigger.
pub fn core_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    system: SystemId,
    anchor: NodeId,
    cores: &[CoreRow],
    actions: &ViewBridge,
) {
    let rows: Vec<(usize, String, bool)> = cores
        .iter()
        .enumerate()
        .filter(|(_, core)| core.system == system)
        .map(|(index, core)| (index, core.name.clone(), core.selected))
        .collect();
    let actions = actions.clone();
    overlays.menu(anchor, move |tree, node| {
        let mut menu = Menu::new(theme);
        for (index, name, selected) in rows.clone() {
            let label = if selected {
                format!("{name} ✓")
            } else {
                name
            };
            let actions = actions.clone();
            menu = menu.item(
                MenuItem::new(label, theme)
                    .on_click(move |_tree, _id| actions.push(Action::SelectCore(index))),
            );
        }
        tree.add_child(node, menu);
    });
}

/// Open the per-game console picker: every console the app knows, with the
/// current one ticked. Picking one overrides the system the file extension
/// suggests (a `.chd` can be a PlayStation or a PSP disc).
pub fn system_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    game_id: i64,
    current: SystemId,
    position: Vec2,
    actions: &ViewBridge,
) {
    // Replaces the card menu this was opened from.
    overlays.close_all();
    let actions = actions.clone();
    overlays.menu_at(position, move |tree, node| {
        let mut menu = Menu::new(theme);
        for system in SYSTEMS {
            let label = if *system == current {
                format!("{} ✓", system.name())
            } else {
                system.name().to_string()
            };
            let actions = actions.clone();
            let system = *system;
            menu = menu.item(MenuItem::new(label, theme).on_click(move |_tree, _id| {
                actions.push(Action::SetGameSystem {
                    id: game_id,
                    system,
                })
            }));
        }
        tree.add_child(node, menu);
    });
}

/// Open the per-game core picker: every core that runs the game's console,
/// with the game's own pick ticked. The first item clears the override so the
/// console's pick in Settings applies again. `game.core` is the game's stored
/// core key (`None` when it follows the console).
pub fn game_core_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    game: &GameRow,
    position: Vec2,
    cores: &[CoreRow],
    actions: &ViewBridge,
) {
    let game_id = game.id;
    let rows: Vec<(String, String)> = cores
        .iter()
        .filter(|core| core.system == game.system)
        .map(|core| (core.key.clone(), core.name.clone()))
        .collect();
    // A stored key that no longer names a core for this console (it was
    // removed, or the console changed) reads as "follow the console's pick".
    let picked = game
        .core
        .clone()
        .filter(|current| rows.iter().any(|(key, _)| key == current));
    // Replaces the card menu this was opened from.
    overlays.close_all();
    let actions = actions.clone();
    overlays.menu_at(position, move |tree, node| {
        let mut menu = Menu::new(theme);
        let default_label = if picked.is_none() {
            "默认（跟随机种设置）✓".to_string()
        } else {
            "默认（跟随机种设置）".to_string()
        };
        menu = menu.item({
            let actions = actions.clone();
            MenuItem::new(default_label, theme).on_click(move |_tree, _id| {
                actions.push(Action::SetGameCore {
                    id: game_id,
                    core: None,
                })
            })
        });
        for (key, name) in rows.clone() {
            let label = if picked.as_deref() == Some(key.as_str()) {
                format!("{name} ✓")
            } else {
                name
            };
            let actions = actions.clone();
            menu = menu.item(MenuItem::new(label, theme).on_click(move |_tree, _id| {
                actions.push(Action::SetGameCore {
                    id: game_id,
                    core: Some(key.clone()),
                })
            }));
        }
        tree.add_child(node, menu);
    });
}

/// Open a modal confirmation for a destructive action. Confirming runs
/// `on_confirm`; Escape / clicking outside cancels.
pub fn confirm_destructive(
    overlays: &mut Overlays,
    title: String,
    message: String,
    on_confirm: Action,
    actions: &ViewBridge,
) {
    let id = overlays.confirm(title, message);
    overlays.destructive(id, true);
    let actions = actions.clone();
    overlays.on_confirm(id, move || actions.push(on_confirm.clone()));
}

/// Open a modal confirmation that is not destructive (e.g. offering to
/// download a core). Confirming runs `on_confirm`; Escape / clicking outside
/// cancels.
pub fn confirm_action(
    overlays: &mut Overlays,
    title: String,
    message: String,
    on_confirm: Action,
    actions: &ViewBridge,
) {
    let id = overlays.confirm(title, message);
    let actions = actions.clone();
    overlays.on_confirm(id, move || actions.push(on_confirm.clone()));
}
