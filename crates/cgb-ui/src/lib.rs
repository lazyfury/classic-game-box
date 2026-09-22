//! The UI: quill views built from a pure [`ViewModel`].
//!
//! `cgb-ui` is the only crate with a window-shaped API, and it still does not
//! know about libretro or the platform. The app projects core state into a
//! [`ViewModel`], builds the tree, drains [`Action`]s, and never reaches into
//! the tree from a callback.
//!
//! See `docs/architecture/quill-native-migration.md` §7 and the quill
//! `docs/ui-guide.md` for the frame loop this wraps.

mod frame;
mod model;
mod view;

pub use frame::{centered_fit, integer_fit, FrameImage};
pub use model::{Action, FrameHandle, GameRow, Section, ViewModel};
pub use view::Actions;

use std::rc::Rc;

use draw_core::{InputEvent, ViewportSize};
use draw_render::PaintContext;
use draw_scene::SceneTree;
use draw_theme::Theme;
use draw_ui::TextMeasurer;

/// The mounted tree plus the frame-loop calls.
pub struct Ui {
    tree: SceneTree,
}

impl Ui {
    /// Build a fresh tree from the model.
    pub fn new(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Self {
        Self {
            tree: view::build(theme, model, actions),
        }
    }

    /// Replace the tree with a rebuilt one (call when the model changed).
    pub fn rebuild(&mut self, theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) {
        self.tree = view::build(theme, model, actions);
    }

    /// The mounted tree.
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// The mounted tree, mutable.
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        &mut self.tree
    }

    /// Install the backend's real font metrics so layout measures what is
    /// painted. Call after [`Ui::new`] and after every [`Ui::rebuild`].
    pub fn install_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        draw_ui::set_text_measurer(&mut self.tree, measurer);
    }

    /// Resolve geometry and flush deferred tree work.
    pub fn layout(&mut self, viewport: ViewportSize) {
        draw_ui::layout(&mut self.tree, viewport);
        self.tree.update();
    }

    /// Emit this frame's draw list into `ctx`.
    pub fn paint(&self, ctx: &mut PaintContext) {
        draw_ui::paint(&self.tree, ctx);
    }

    /// Route one input event. The app maps platform events to
    /// [`InputEvent`] first.
    pub fn route_input(&mut self, event: &InputEvent) {
        let _ = draw_ui::route_input(&mut self.tree, event);
    }
}
