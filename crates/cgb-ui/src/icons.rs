//! Vector icons for the card controls, drawn from vendored Lucide SVGs.
//!
//! Rendering goes through `draw_svg`: the SVG is flattened into `draw_render`
//! lines and stroked, so an icon needs no texture and no extra backend. Each
//! source is embedded with `include_str!` (no runtime asset path), parsed once
//! per thread and cached.
//!
//! The SVGs are from [Lucide](https://lucide.dev) (`assets/icons/`, ISC — see
//! the `LICENSE` beside them). `stroke="currentColor"` is resolved to the
//! colour passed to [`Icon::new`], which is exactly the hook an icon needs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use draw_components::{Component, Spec};
use draw_core::{Color, Rect, Size};
use draw_render::PaintContext;
use draw_svg::SvgDocument;
use draw_ui::{InteractState, MouseFilter, Widget};

/// Which vendored icon to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// A pushpin; the card's pin toggle.
    Pin,
    /// A trash can; the card's delete button.
    Trash,
    /// An up arrow; ascending sort.
    ArrowUp,
    /// A down arrow; descending sort.
    ArrowDown,
}

impl IconName {
    /// The embedded SVG source for this icon.
    fn source(self) -> &'static str {
        match self {
            IconName::Pin => include_str!("../assets/icons/pin.svg"),
            IconName::Trash => include_str!("../assets/icons/trash-2.svg"),
            IconName::ArrowUp => include_str!("../assets/icons/arrow-up.svg"),
            IconName::ArrowDown => include_str!("../assets/icons/arrow-down.svg"),
        }
    }
}

thread_local! {
    /// Parsed icons, keyed by name. Parsing is cheap but not free, and cards
    /// are rebuilt on every model change, so the result is kept per thread
    /// (the UI is single-threaded).
    static CACHE: RefCell<HashMap<IconName, Rc<SvgDocument>>> = RefCell::new(HashMap::new());
}

/// The parsed (and cached) document for an icon, or `None` if it failed to
/// parse.
fn document(name: IconName) -> Option<Rc<SvgDocument>> {
    CACHE.with(|cache| {
        if let Some(document) = cache.borrow().get(&name) {
            return Some(document.clone());
        }
        let document = Rc::new(SvgDocument::parse(name.source()).ok()?);
        cache.borrow_mut().insert(name, document.clone());
        Some(document)
    })
}

/// A `size × size` icon component, stroked in `color` and centred in its cell.
///
/// The size is explicit, not inferred from the parent rectangle, so a compact
/// button does not shrink the icon. `mouse_filter` is `Ignore`, so a click
/// lands on the surrounding button.
pub struct Icon {
    spec: Spec,
    name: IconName,
    color: Color,
    size: f32,
}

impl Icon {
    pub fn new(name: IconName, color: Color, size: f32) -> Self {
        Self {
            spec: Spec::leaf(),
            name,
            color,
            size,
        }
    }
}

impl Component for Icon {
    fn spec(&mut self) -> &mut Spec {
        &mut self.spec
    }

    fn name(&self) -> &'static str {
        "Icon"
    }

    fn widget(&self) -> Widget {
        Widget::Panel {
            color: Color::TRANSPARENT,
            border: None,
        }
    }

    fn prepare(&mut self) {
        self.spec.data.min_size = Size::splat(self.size);
        self.spec.data.mouse_filter = MouseFilter::Ignore;

        let Some(document) = document(self.name) else {
            return;
        };
        let color = self.color;
        let size = self.size;
        self.spec.foreground = Some(Box::new(
            move |ctx: &mut PaintContext, rect: Rect, _state: InteractState| {
                let target = Rect::from_center_size(rect.center(), Size::splat(size));
                document.draw(ctx, target, color);
            },
        ));
    }
}

draw_components::impl_scene_child!(Icon);

#[cfg(test)]
mod tests {
    use super::*;
    use draw_core::{Rect, Vec2};
    use draw_render::PaintContext;

    #[test]
    fn every_icon_parses_and_draws() {
        for name in [
            IconName::Pin,
            IconName::Trash,
            IconName::ArrowUp,
            IconName::ArrowDown,
        ] {
            let document = document(name).expect("icon parses");
            let mut ctx = PaintContext::new();
            document.draw(
                &mut ctx,
                Rect::from_min_size(Vec2::ZERO, Size::splat(24.0)),
                Color::WHITE,
            );
            assert!(!ctx.into_draw_list().is_empty(), "{name:?} drew nothing");
        }
    }
}
