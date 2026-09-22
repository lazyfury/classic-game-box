//! The framebuffer → screen path.
//!
//! `draw_render` already has `DrawCommand::DrawImage`, and the wgpu backend can
//! stream a texture with `update_texture` + `TextureFilter::Nearest`. What the
//! UI stack lacks is an *image widget*: `draw_ui::Widget` is a closed enum with
//! no image variant and `draw_components` ships no `Image`.
//!
//! Rather than fork quill's widget enum, this module builds a leaf component on
//! quill's public extension points — `draw_components::Component` plus the
//! `foreground` decorator (the same hook `Divider` uses). The component carries
//! the texture handle, reserves the available area, and paints the frame at an
//! integer scale, letterboxed and centred, from the rectangle layout hands it.
//! Nothing here knows about libretro: it is handed a `TextureId`.
//!
//! Git history: the clean long-term home for this is a `Widget::Image` +
//! `draw_components::Image` pair in quill; the app-local component unblocks Q1
//! without touching a sibling repo's public API.

use draw_components::base::{Component, Spec};
use draw_core::{Color, Rect, Size, Vec2};
use draw_render::{Paint, TextureId};
use draw_ui::{MouseFilter, Widget};

/// The largest integer scale that fits `frame` inside `available`, and the
/// resulting size in logical pixels.
///
/// Returns `(width, height)` at the integer scale. When the frame is bigger
/// than the area the scale floors to 1, so the caller should clip; upscaling is
/// never fractional.
pub fn integer_fit(frame: (u32, u32), available: (f32, f32)) -> (f32, f32) {
    let (fw, fh) = frame;
    let (aw, ah) = available;
    if fw == 0 || fh == 0 {
        return (0.0, 0.0);
    }
    let scale_x = (aw / fw as f32).floor();
    let scale_y = (ah / fh as f32).floor();
    let scale = scale_x.min(scale_y).max(1.0);
    (fw as f32 * scale, fh as f32 * scale)
}

/// The integer-scaled frame rectangle, centred inside `area`.
///
/// This is the destination the image is drawn into: pixels stay whole, and the
/// slack becomes even letterbox/pillarbox bands around the picture.
pub fn centered_fit(frame: (u32, u32), area: Rect) -> Rect {
    let (width, height) = integer_fit(frame, (area.size.width, area.size.height));
    let origin = Vec2::new(
        area.left() + (area.size.width - width) * 0.5,
        area.top() + (area.size.height - height) * 0.5,
    );
    Rect::from_min_size(origin, Size::new(width, height))
}

/// A leaf component that paints a registered framebuffer texture.
///
/// Give it the texture handle from [`FrameHandle`](crate::FrameHandle) and let
/// it grow; at paint time it computes the integer-scaled destination inside
/// whatever rectangle the layout assigned and emits one `DrawImage`.
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

    fn widget(&self) -> Widget {
        // A transparent, borderless panel: the visual is the foreground image.
        Widget::Panel {
            color: Color::TRANSPARENT,
            border: None,
        }
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
    fn a_nes_frame_scales_by_whole_pixels() {
        // 256x240 in a wide area: height is the limit.
        assert_eq!(integer_fit((256, 240), (1920.0, 720.0)), (768.0, 720.0));
    }

    #[test]
    fn a_frame_larger_than_the_area_floors_to_one() {
        assert_eq!(integer_fit((256, 240), (128.0, 120.0)), (256.0, 240.0));
    }

    #[test]
    fn a_gba_frame_letterboxes() {
        // 240x160 in a square area: width is the limit (scale 3).
        assert_eq!(integer_fit((240, 160), (800.0, 800.0)), (720.0, 480.0));
    }

    #[test]
    fn centered_fit_puts_the_slack_in_even_bands() {
        let area = Rect::from_min_size(Vec2::new(100.0, 50.0), Size::new(800.0, 800.0));
        let fitted = centered_fit((240, 160), area);
        assert_eq!(fitted.size, Size::new(720.0, 480.0));
        assert_eq!(fitted.origin, Vec2::new(140.0, 210.0));
    }
}
