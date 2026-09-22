//! The framebuffer → screen path — and the one quill gap it hits.
//!
//! `draw_render` already has `DrawCommand::DrawImage`, and the wgpu backend can
//! stream a texture with `update_texture` + `TextureFilter::Nearest`. But the
//! **UI layer has no image widget**: `draw_ui::Widget` covers `Panel`, `Label`,
//! `Button`, `Flex` and `Grid`, and `draw_components` has no `Image`. So a view
//! cannot emit a `DrawImage` through the tree yet.
//!
//! Two ways forward, both small:
//!
//! 1. **Add an `Image` widget to quill** (`draw_ui::Widget::Image { texture,
//!    source, size }`, painted with `PaintContext::draw_image`, plus a
//!    `draw_components::Image`). This is the clean fix and belongs in quill.
//! 2. **App-side workaround for Q1**: paint the emulator frame into the
//!    `PaintContext` *before* the UI, using a destination rect the app computes
//!    from [`integer_fit`] and the window size. The UI then draws on top.
//!
//! Either way, the geometry rule is here: emulator pixels stay integer-scaled
//! (the old front end's "a game pixel is always a whole number of physical
//! pixels" rule), letterboxed inside the available area.

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
}
