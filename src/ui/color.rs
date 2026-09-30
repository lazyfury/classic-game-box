//! Colours for artwork and cover placeholders.
//!
//! [`media`] is the ink/scrim palette painted *over* game artwork (covers,
//! screenshots), independent of the light/dark theme. [`cover_color`] gives a
//! game without artwork a stable colour drawn from its ROM path.

use igui::igui_core::Color;

/// Colors painted over game artwork (covers, screenshots), not over a theme
/// surface. They are deliberately independent of light/dark: the scrim is dark
/// and the labels light so they stay legible against arbitrary imagery.
/// Centralised so the badge and the controls cannot drift apart.
pub(crate) mod media {
    use igui::igui_core::Color;

    /// A translucent dark scrim behind labels on artwork.
    pub(crate) const SCRIM: Color = Color::new(0.0, 0.0, 0.0, 0.4);
    /// The small surface behind a console badge on a cover.
    pub(crate) const BADGE: Color = Color::new(0.11, 0.11, 0.13, 0.72);
    /// Primary ink on artwork.
    pub(crate) const ON_MEDIA: Color = Color::new(1.0, 1.0, 1.0, 0.92);
    /// Secondary ink on artwork (the control icons and badge label).
    pub(crate) const ON_MEDIA_MUTED: Color = Color::new(1.0, 1.0, 1.0, 0.85);
    /// Hover fill for a control sitting on artwork.
    pub(crate) const ON_MEDIA_HOVER: Color = Color::new(1.0, 1.0, 1.0, 0.16);
    /// Dark ink for a light artwork placeholder.
    pub(crate) const ON_MEDIA_DARK: Color = Color::new(0.07, 0.07, 0.09, 0.92);
}

/// A stable, readable cover colour for a ROM path: hash it to a hue with a
/// fixed saturation and value, so the whole grid stays legible against light
/// text. FNV-1a, because it is short and stable across runs.
pub(crate) fn cover_color(path: &str) -> Color {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hsv((hash % 360) as f32, 0.45, 0.45)
}

/// Ink that stays legible on a cover color: dark on a light hue, light on a
/// dark one.
pub(crate) fn cover_ink(color: Color) -> Color {
    let [r, g, b, _] = color.to_rgba8();
    let luminance = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
    if luminance > 0.55 {
        media::ON_MEDIA_DARK
    } else {
        media::ON_MEDIA
    }
}

/// HSV (h in degrees) to an RGB [`Color`]. Only used for cover hues.
fn hsv(hue: f32, saturation: f32, value: f32) -> Color {
    let chroma = value * saturation;
    let h = hue / 60.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    Color::rgb(r + m, g + m, b + m)
}
