//! Context menus and the destructive confirmation dialog, built on the overlay
//! layer (`igui_components::Overlays`).
//!
//! These live in the view layer, not the host: the menu items, their labels and
//! the actions they push are view content. [`Ui`](crate::ui::Ui) only owns the
//! overlay layer and asks for `repaint`, so the host stays free of view code.

use igui::igui_components::{Column, Component, Menu, MenuItem, OverlayId, Overlays, Row, Text};
use igui::igui_core::{NodeId, Vec2};
use igui::igui_theme::{space, Theme, Tone};

use cgb_libretro::{KeyboardMode, SystemId, SYSTEMS};

use crate::host::GamepadDevice;

use crate::ui::model::{Action, Confirm, CoreRow, GameRow};

use super::ViewBridge;

/// How many ports the assignment menu offers per gamepad.
const ASSIGN_PORTS: usize = 4;

/// Open the input-assignment menu: the connected gamepads, each showing its
/// current port and opening a port picker. Anchored at the play-view button.
pub fn input_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    anchor: NodeId,
    devices: Vec<GamepadDevice>,
    keyboard_mode: KeyboardMode,
    actions: &ViewBridge,
) {
    let actions = actions.clone();
    overlays.menu(anchor, move |tree, node| {
        let mut menu = Menu::new(theme)
            .item(MenuItem::new(format!("键盘：{}", keyboard_mode.label()), theme).disabled(true));
        if devices.is_empty() {
            menu = menu
                .item(MenuItem::new("没有检测到手柄。连上手柄后重新打开。", theme).disabled(true));
        } else {
            menu = menu.separator();
            for (index, device) in devices.iter().enumerate() {
                let current = match device.port {
                    Some(port) => format!("{}P", port + 1),
                    None => "未分配".to_string(),
                };
                let actions = actions.clone();
                let id = device.id.clone();
                let name = device.name.clone();
                menu = menu.item(
                    MenuItem::new(format!("手柄 {}：{}", index + 1, device.name), theme)
                        .shortcut(current)
                        .on_click(move |_tree, _id| {
                            actions.push(Action::OpenInputPorts {
                                id: id.clone(),
                                name: name.clone(),
                                anchor,
                            })
                        }),
                );
            }
        }
        let mode = actions.clone();
        menu = menu.separator().item(
            MenuItem::new("分配模式（按 Start 认领）…", theme)
                .on_click(move |_tree, _id| mode.push(Action::OpenAssignMode)),
        );
        tree.add_child(node, menu);
    });
}

/// Open the "press Start to claim" assignment modal: the connected gamepads
/// and the port each drives, finished with the confirm row.
pub fn assign_modal(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    devices: Vec<GamepadDevice>,
    waiting: bool,
    confirming: bool,
) -> OverlayId {
    overlays.close_all();
    let id = overlays.modal("分配模式", move |tree, node| {
        let mut column = Column::new().gap(space::SM);
        let (hint, tone) = if confirming {
            ("再长按主控 Select 确认重置全部？", Tone::Error)
        } else if waiting {
            ("等待手柄反馈：按 Start 认领", Tone::Default)
        } else {
            ("没有可分配的空位", Tone::Default)
        };
        column = column.child(Text::small(hint, theme).tone(tone));
        column = column.child(Text::small("手柄列表", theme).tone(Tone::Muted));
        if devices.is_empty() {
            column = column.child(Text::small("没有检测到手柄。", theme));
        }
        for device in &devices {
            let name = if device.name.is_empty() {
                format!("手柄 {}", device.id)
            } else {
                format!("{}（{}）", device.name, device.id)
            };
            column = column.child(
                Row::new()
                    .gap(space::SM)
                    .child(Text::small(name, theme).grow(1.0))
                    .child(Text::small(port_label(device.port), theme).tone(Tone::Muted)),
            );
        }
        column =
            column.child(Text::caption("主控（1P）长按 Select 重置全部", theme).tone(Tone::Muted));
        tree.add_child(node, column);
    });
    overlays.confirm_label(id, "确认完成分配");
    overlays.cancel_label(id, "取消");
    id
}

/// The port as the assignment modal shows it: `1P` is the main controller.
fn port_label(port: Option<usize>) -> String {
    match port {
        Some(0) => "1P（主控）".to_string(),
        Some(port) => format!("{}P", port + 1),
        None => "未分配".to_string(),
    }
}

/// Open the port picker for one gamepad: `1P`..`4P` and unassign, with the
/// current port ticked, and a row back to the gamepad list. Anchored at the
/// play-view button.
pub fn input_ports_menu(
    theme: &'static dyn Theme,
    overlays: &mut Overlays,
    anchor: NodeId,
    id: String,
    name: String,
    current: Option<usize>,
    actions: &ViewBridge,
) {
    let actions = actions.clone();
    overlays.menu(anchor, move |tree, node| {
        let mut menu = Menu::new(theme)
            .item(MenuItem::new(format!("手柄：{name}"), theme).disabled(true))
            .separator();
        for port in 0..ASSIGN_PORTS {
            let label = if current == Some(port) {
                format!("{}P ✓", port + 1)
            } else {
                format!("{}P", port + 1)
            };
            let actions = actions.clone();
            let id = id.clone();
            menu = menu.item(MenuItem::new(label, theme).on_click(move |_tree, _id| {
                actions.push(Action::AssignInput {
                    id: id.clone(),
                    port: Some(port),
                })
            }));
        }
        let unassigned = if current.is_none() {
            "未分配 ✓"
        } else {
            "未分配"
        };
        let assign = actions.clone();
        let assign_id = id.clone();
        let back = actions.clone();
        menu = menu
            .item(
                MenuItem::new(unassigned, theme).on_click(move |_tree, _id| {
                    assign.push(Action::AssignInput {
                        id: assign_id.clone(),
                        port: None,
                    })
                }),
            )
            .separator()
            .item(
                MenuItem::new("返回", theme)
                    .on_click(move |_tree, _id| back.push(Action::OpenInputAssign { anchor })),
            );
        tree.add_child(node, menu);
    });
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
