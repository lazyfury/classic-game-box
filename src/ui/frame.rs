//! The framebuffer → screen path.
//!
//! ``igui_render`` already has `DrawCommand::DrawImage`, and the wgpu backend can
//! stream a texture with `update_texture` + `TextureFilter::Nearest`. What the
//! UI stack lacks is an *image widget*: `igui::igui_ui::Widget` is a closed enum with
//! no image variant and ``igui_components`` ships no `Image`.
//!
//! Rather than fork quill's widget enum, this module builds a leaf component on
//! quill's public extension points — `igui::igui_components::Component` plus the
//! `foreground` decorator (the same hook `Divider` uses). The component carries
//! the texture handle, reserves the available area, and paints the frame scaled
//! to fill the area's limiting dimension, centred, from the rectangle layout
//! hands it. Nothing here knows about libretro: it is handed a `TextureId`.
//!
//! Git history: the clean long-term home for this is a `Widget::Image` +
//! `igui::igui_components::Image` pair in quill; the app-local component unblocks Q1
//! without touching a sibling repo's public API.

use igui::igui_components::base::{Component, Spec};
use igui::igui_core::{Rect, Size, Vec2};
use igui::igui_render::{Paint, TextureId};
use igui::igui_ui::MouseFilter;

/// The size `frame` takes when scaled to fit `available`, keeping its aspect
/// ratio: the limiting dimension is filled exactly and the other is left short.
///
/// Fractional, so the picture is as large as the area allows rather than
/// snapping down to the nearest whole-pixel multiple. Both dimensions are
/// floored to whole logical pixels so the result never spills past the area.
pub fn contain_fit(frame: (u32, u32), available: (f32, f32)) -> (f32, f32) {
    let (fw, fh) = frame;
    let (aw, ah) = available;
    if fw == 0 || fh == 0 || aw <= 0.0 || ah <= 0.0 {
        return (0.0, 0.0);
    }
    let scale = (aw / fw as f32).min(ah / fh as f32);
    ((fw as f32 * scale).floor(), (fh as f32 * scale).floor())
}

/// The fitted frame rectangle, centred inside `area`.
///
/// The destination the image is drawn into: the shorter dimension is filled and
/// the slack becomes even letterbox/pillarbox bands around the picture.
pub fn centered_fit(frame: (u32, u32), area: Rect) -> Rect {
    let (width, height) = contain_fit(frame, (area.size.width, area.size.height));
    let origin = Vec2::new(
        area.left() + (area.size.width - width) * 0.5,
        area.top() + (area.size.height - height) * 0.5,
    );
    Rect::from_min_size(origin, Size::new(width, height))
}

/// The rectangle that makes `frame` *fill* `area`, cropping the overflow (the
/// CSS `object-fit: cover`). The destination is at least as large as `area` on
/// both axes, centred; the caller clips it to `area`.
pub fn cover_fit(frame: (u32, u32), area: Rect) -> Rect {
    let (fw, fh) = frame;
    if fw == 0 || fh == 0 || area.size.width <= 0.0 || area.size.height <= 0.0 {
        return Rect::from_min_size(area.origin, Size::ZERO);
    }
    let scale = (area.size.width / fw as f32).max(area.size.height / fh as f32);
    let size = Size::new(fw as f32 * scale, fh as f32 * scale);
    let origin = Vec2::new(
        area.left() + (area.size.width - size.width) * 0.5,
        area.top() + (area.size.height - size.height) * 0.5,
    );
    Rect::from_min_size(origin, size)
}

/// A leaf component that paints a registered framebuffer texture.
///
/// Give it the texture handle from [`FrameHandle`](crate::ui::FrameHandle) and let
/// it grow; at paint time it computes the fitted destination inside whatever
/// rectangle the layout assigned and emits one `DrawImage`.
pub struct FrameImage {
    spec: Spec,
    texture: TextureId,
    frame: (u32, u32),
}

impl FrameImage {
    /// A frame of `texture`, whose source is `width`×`height` pixels.
    pub fn new(texture: TextureId, width: u32, height: u32) -> Self {
        Self {
            spec: Spec::default(),
            texture,
            frame: (width, height),
        }
    }
}

impl Component for FrameImage {
    fn spec(&mut self) -> &mut Spec {
        &mut self.spec
    }

    fn name(&self) -> &'static str {
        "FrameImage"
    }

    fn prepare(&mut self) {
        let texture = self.texture;
        let frame = self.frame;
        self.spec.data.mouse_filter = MouseFilter::Ignore;
        self.spec.foreground = Some(Box::new(move |ctx, rect, _state| {
            let destination = centered_fit(frame, rect);
            ctx.draw_image(texture, destination, None, Paint::default());
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nes_frame_fills_the_short_edge() {
        // 256x240 in a wide area: height is the limit and fills exactly.
        assert_eq!(contain_fit((256, 240), (1920.0, 720.0)), (768.0, 720.0));
    }

    #[test]
    fn a_frame_larger_than_the_area_shrinks_to_fit() {
        assert_eq!(contain_fit((256, 240), (128.0, 120.0)), (128.0, 120.0));
    }

    #[test]
    fn a_gba_frame_pillarboxes_at_a_fractional_scale() {
        // 240x160 in a square area: width fills, height is 160 * (800/240).
        assert_eq!(contain_fit((240, 160), (800.0, 800.0)), (800.0, 533.0));
    }

    #[test]
    fn centered_fit_puts_the_slack_in_even_bands() {
        let area = Rect::from_min_size(Vec2::new(100.0, 50.0), Size::new(800.0, 800.0));
        let fitted = centered_fit((240, 160), area);
        assert_eq!(fitted.size, Size::new(800.0, 533.0));
        assert_eq!(fitted.origin, Vec2::new(100.0, 183.5));
    }

    #[test]
    fn cover_fit_fills_the_area_and_crops_the_overflow() {
        let area = Rect::from_min_size(Vec2::new(10.0, 20.0), Size::new(100.0, 100.0));
        // A 4:3 frame in a square area: the height fills, the width overflows.
        let fitted = cover_fit((256, 240), area);
        assert!((fitted.size.height - 100.0).abs() < 0.001);
        assert!(fitted.size.width > 100.0);
        assert!(fitted.origin.x < area.left(), "cropped on both sides");
        assert!((fitted.origin.y - area.top()).abs() < 0.001);
    }
}
