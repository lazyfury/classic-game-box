//! Keyboard focus for the mounted tree.
//!
//! igui's own keyboard activation only fires for its low-level `Widget::Button`,
//! which the themed components and this app's custom targets do not use, so the
//! host drives focus and activation here. These are free functions over the
//! [`SceneTree`] so [`Ui`](crate::ui::Ui) can stay a thin wrapper.

use igui::igui_core::NodeId;
use igui::igui_scene::SceneTree;
use igui::igui_ui::{self, Control};

/// Move keyboard focus to the next (or previous) interactive control, in tree
/// order. Returns `false` when the UI has nothing to focus.
pub fn move_focus(tree: &mut SceneTree, backward: bool) -> bool {
    let order = focus_order(tree);
    if order.is_empty() {
        return false;
    }
    let current = igui_ui::focused(tree);
    let index = current.and_then(|id| order.iter().position(|node| *node == id));
    let next = match index {
        Some(index) if backward => (index + order.len() - 1) % order.len(),
        Some(index) => (index + 1) % order.len(),
        None if backward => order.len() - 1,
        None => 0,
    };
    igui_ui::gui_state_mut(tree).focused = Some(order[next]);
    true
}

/// Activate the focused control the way Enter / Space should: run its click
/// callback. Returns `false` when nothing focused owns a click.
pub fn activate_focus(tree: &mut SceneTree) -> bool {
    let Some(id) = igui_ui::focused(tree) else {
        return false;
    };
    // A disabled control (or one under a disabled ancestor) never fires.
    let mut guard = Some(id);
    while let Some(node) = guard {
        if tree
            .data::<Control>(node)
            .is_some_and(|control| control.data.disabled)
        {
            return false;
        }
        guard = tree.parent(node);
    }
    // The nearest ancestor with a callback owns the click.
    let mut current = Some(id);
    while let Some(node) = current {
        if let Some(callback) = tree
            .data::<Control>(node)
            .and_then(|control| control.callback.clone())
        {
            (callback.borrow_mut())(tree, node);
            return true;
        }
        current = tree.parent(node);
    }
    false
}

/// Keep keyboard focus on a control that is focusable in its own right (a text
/// field). A pointer click lands on the deepest control under it, which for a
/// card or a row is a text label with no click action; snap the focus to the
/// nearest ancestor that has one so the ring and Tab agree.
pub fn normalize(tree: &mut SceneTree) {
    let Some(id) = igui_ui::focused(tree) else {
        return;
    };
    if tree
        .data::<Control>(id)
        .is_some_and(|control| control.focusable)
    {
        return;
    }
    let target = interactive_ancestor(tree, id);
    if target != Some(id) {
        igui_ui::gui_state_mut(tree).focused = target;
    }
}

/// The nearest ancestor of `id` (including itself) with a click callback.
fn interactive_ancestor(tree: &SceneTree, id: NodeId) -> Option<NodeId> {
    let mut current = Some(id);
    while let Some(node) = current {
        if tree
            .data::<Control>(node)
            .is_some_and(|control| control.callback.is_some())
        {
            return Some(node);
        }
        current = tree.parent(node);
    }
    None
}

/// Every focusable control in tree order: the nodes that own a click callback,
/// are enabled, and are not clipped away. (A drag handle has no keyboard
/// equivalent yet, so it is not a focus stop.)
fn focus_order(tree: &SceneTree) -> Vec<NodeId> {
    let mut order = Vec::new();
    collect_focusable(tree, tree.root(), &mut order);
    order
}

fn collect_focusable(tree: &SceneTree, id: NodeId, out: &mut Vec<NodeId>) {
    if let Some(control) = tree.data::<Control>(id) {
        let interactive = control.callback.is_some();
        let clipped = matches!(control.data.clip_rect, Some(rect) if rect.is_empty());
        if interactive && !control.data.disabled && !clipped {
            out.push(id);
        }
    }
    if let Some(children) = tree.children(id) {
        for child in children {
            collect_focusable(tree, *child, out);
        }
    }
}
