//! Context menus and the destructive confirmation dialog, built on the overlay
//! layer (`igui_components::Overlays`).
//!
//! These live in the view layer, not the host: the menu items, their labels and
//! the actions they push are view content. [`Ui`](crate::ui::Ui) only owns the
//! overlay layer and asks for `repaint`, so the host stays free of view code.

use igui::igui_components::{Menu, MenuItem, Overlays};
use igui::igui_core::{NodeId, Vec2};
use igui::igui_theme::Theme;

use cgb_systems::{SystemId, SYSTEMS};

use crate::ui::model::{Action, Confirm, CoreRow, GameRow};

use super::ViewBridge;

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
            MenuItem::new(label, theme).on_click(move |_tree, _id| actions.push(action))
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
    overlays.on_confirm(id, move || actions.push(on_confirm));
}
