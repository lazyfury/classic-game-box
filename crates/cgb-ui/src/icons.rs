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
use draw_core::{Color, Rect, Size, Vec2};
use draw_render::{DrawCommand, Paint, PaintContext};
use draw_svg::SvgDocument;
use draw_ui::{InteractState, MouseFilter, Widget};

use crate::model::FrameHandle;

/// Which vendored icon to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// A pushpin; the card's pin toggle.
    Pin,
    /// A trash can; delete.
    Trash,
    /// An up arrow; ascending sort.
    ArrowUp,
    /// A down arrow; descending sort.
    ArrowDown,
    /// A camera; the screenshots section and the capture button.
    Camera,
    /// A picture with a plus; "set as cover".
    ImagePlus,
    /// A folder with a magnifier; reveal in the file browser.
    FolderSearch,
    /// A star; the cover flag.
    Star,
    /// A cross; close a preview.
    Close,
    /// A left chevron; the previous screenshot.
    ChevronLeft,
    /// A right chevron; the next screenshot.
    ChevronRight,
    /// A stack of books; the library section.
    Library,
    /// Two sliders; the settings section.
    Settings2,
    /// A pencil; rename.
    Pencil,
    /// A tag; edit tags.
    Tag,
    /// A check mark; commit an edit.
    Check,
    /// A floppy disk; the saves section.
    Save,
}

impl IconName {
    /// Every icon in the pack, for tests and iteration.
    pub const ALL: [IconName; 17] = [
        IconName::Pin,
        IconName::Trash,
        IconName::ArrowUp,
        IconName::ArrowDown,
        IconName::Camera,
        IconName::ImagePlus,
        IconName::FolderSearch,
        IconName::Star,
        IconName::Close,
        IconName::ChevronLeft,
        IconName::ChevronRight,
        IconName::Library,
        IconName::Settings2,
        IconName::Pencil,
        IconName::Tag,
        IconName::Check,
        IconName::Save,
    ];

    /// The embedded SVG source for this icon.
    fn source(self) -> &'static str {
        match self {
            IconName::Pin => include_str!("../assets/icons/pin.svg"),
            IconName::Trash => include_str!("../assets/icons/trash-2.svg"),
            IconName::ArrowUp => include_str!("../assets/icons/arrow-up.svg"),
            IconName::ArrowDown => include_str!("../assets/icons/arrow-down.svg"),
            IconName::Camera => include_str!("../assets/icons/camera.svg"),
            IconName::ImagePlus => include_str!("../assets/icons/image-plus.svg"),
            IconName::FolderSearch => include_str!("../assets/icons/folder-search.svg"),
            IconName::Star => include_str!("../assets/icons/star.svg"),
            IconName::Close => include_str!("../assets/icons/x.svg"),
            IconName::ChevronLeft => include_str!("../assets/icons/chevron-left.svg"),
            IconName::ChevronRight => include_str!("../assets/icons/chevron-right.svg"),
            IconName::Library => include_str!("../assets/icons/library.svg"),
            IconName::Settings2 => include_str!("../assets/icons/settings-2.svg"),
            IconName::Pencil => include_str!("../assets/icons/pencil.svg"),
            IconName::Tag => include_str!("../assets/icons/tag.svg"),
            IconName::Check => include_str!("../assets/icons/check.svg"),
            IconName::Save => include_str!("../assets/icons/save.svg"),
        }
    }
}

thread_local! {
    /// Parsed icons, keyed by name. Parsing is cheap but not free, and cards
    /// are rebuilt on every model change, so the result is kept per thread
    /// (the UI is single-threaded).
    static CACHE: RefCell<HashMap<IconName, Rc<SvgDocument>>> = RefCell::new(HashMap::new());

    /// Rasterized icon textures, registered by the host. When an icon has one,
    /// it is drawn as a single image instead of re-stroking its SVG every
    /// frame — a card's controls are most of a frame's draw commands.
    static TEXTURES: RefCell<HashMap<IconName, FrameHandle>> = RefCell::new(HashMap::new());
}

/// Register a rasterized texture for an icon (see [`rasterize_icon`]). The host
/// calls this once after the backend exists.
pub fn set_texture(name: IconName, handle: FrameHandle) {
    TEXTURES.with(|textures| textures.borrow_mut().insert(name, handle));
}

/// Drop every registered icon texture (used by tests).
pub fn clear_textures() {
    TEXTURES.with(|textures| textures.borrow_mut().clear());
}

fn texture(name: IconName) -> Option<FrameHandle> {
    TEXTURES.with(|textures| textures.borrow().get(&name).copied())
}

/// Rasterize an icon to white RGBA8, `size_px` square, ready for
/// `register_texture`. The stroke is anti-aliased; the host tints it at draw
/// time, so one texture serves every colour. `None` if the SVG fails to parse.
pub fn rasterize_icon(name: IconName, size_px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let document = document(name)?;
    if size_px == 0 {
        return None;
    }
    let target = Rect::from_min_size(Vec2::ZERO, Size::splat(size_px as f32));
    let mut ctx = PaintContext::new();
    document.draw(&mut ctx, target, Color::WHITE);
    let list = ctx.into_draw_list();

    let side = size_px as usize;
    let mut coverage = vec![0.0f32; side * side];
    for command in list.commands() {
        match command {
            DrawCommand::Line {
                from, to, width, ..
            } => raster_segment(&mut coverage, side, *from, *to, width * 0.5),
            DrawCommand::FillCircle { center, radius, .. } => {
                raster_disc(&mut coverage, side, *center, *radius)
            }
            _ => {}
        }
    }

    let mut rgba = vec![0u8; side * side * 4];
    for (index, cover) in coverage.iter().enumerate() {
        rgba[index * 4] = 255;
        rgba[index * 4 + 1] = 255;
        rgba[index * 4 + 2] = 255;
        rgba[index * 4 + 3] = (cover.clamp(0.0, 1.0) * 255.0) as u8;
    }
    Some((size_px, size_px, rgba))
}

/// A thick line segment into the coverage buffer, anti-aliased at the edges.
fn raster_segment(coverage: &mut [f32], side: usize, from: Vec2, to: Vec2, half_width: f32) {
    let (min_x, max_x, min_y, max_y) = bounds(side, from, to, half_width);
    let edge = to - from;
    let length_sq = edge.x * edge.x + edge.y * edge.y;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let offset = point - from;
            let t = if length_sq > 0.0 {
                ((offset.x * edge.x + offset.y * edge.y) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let closest = from + edge * t;
            let distance = ((point.x - closest.x).powi(2) + (point.y - closest.y).powi(2)).sqrt();
            let cover = (half_width + 0.5 - distance).clamp(0.0, 1.0);
            let slot = &mut coverage[y * side + x];
            if cover > *slot {
                *slot = cover;
            }
        }
    }
}

/// A filled circle into the coverage buffer (round caps and joins).
fn raster_disc(coverage: &mut [f32], side: usize, center: Vec2, radius: f32) {
    let (min_x, max_x, min_y, max_y) = bounds(side, center, center, radius);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let distance = ((point.x - center.x).powi(2) + (point.y - center.y).powi(2)).sqrt();
            let cover = (radius + 0.5 - distance).clamp(0.0, 1.0);
            let slot = &mut coverage[y * side + x];
            if cover > *slot {
                *slot = cover;
            }
        }
    }
}

/// The pixel bounds of a primitive, padded for anti-aliasing.
fn bounds(side: usize, a: Vec2, b: Vec2, pad: f32) -> (usize, usize, usize, usize) {
    let min_x = (a.x.min(b.x) - pad - 1.0).floor().max(0.0) as usize;
    let max_x = (a.x.max(b.x) + pad + 1.0).ceil().min(side as f32) as usize;
    let min_y = (a.y.min(b.y) - pad - 1.0).floor().max(0.0) as usize;
    let max_y = (a.y.max(b.y) + pad + 1.0).ceil().min(side as f32) as usize;
    (min_x, max_x, min_y, max_y)
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
        let color = self.color;
        let size = self.size;

        // A registered texture is one draw command; the vector fallback is many
        // (used until the host installs textures, and by tests).
        if let Some(handle) = texture(self.name) {
            self.spec.foreground = Some(Box::new(
                move |ctx: &mut PaintContext, rect: Rect, _state: InteractState| {
                    let target = Rect::from_center_size(rect.center(), Size::splat(size));
                    ctx.draw_image(handle.texture, target, None, Paint::new(color));
                },
            ));
            return;
        }

        let Some(document) = document(self.name) else {
            return;
        };
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
    fn rasterizing_an_icon_fills_some_pixels() {
        let (width, height, rgba) = rasterize_icon(IconName::Camera, 32).expect("rasterize");
        assert_eq!((width, height), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let covered = rgba.chunks(4).filter(|pixel| pixel[3] > 0).count();
        assert!(covered > 0, "the icon covers some pixels");
        assert!(covered < 32 * 32, "but not every pixel");
    }

    #[test]
    fn every_icon_parses_and_draws() {
        for name in IconName::ALL {
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
