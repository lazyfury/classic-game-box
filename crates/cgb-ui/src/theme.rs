//! A custom [`Theme`] for Classic Game Box — the house style, applied without
//! forking the component library.
//!
//! `igui::igui_theme::Theme` derives every token from `palette()` + `mode()`, so a
//! custom look is one `impl Theme` that overrides only the tokens that carry
//! the brand. This module overrides four:
//!
//! 1. **Palette** — a warm arcade-amber accent over a neutral base, with the
//!    light/dark neutrals owned here instead of the library's defaults.
//! 2. **`surface`** — overlays are opaque and flat: `Floating` is the content
//!    surface, separated by a 1 px border, never a frosted layer.
//! 3. **`font_weight`** — headings render bold.
//! 4. **`radius`** — restrained geometry: 4–8 px, never a large rounded card.
//!
//! Nothing in ``igui_ui`` / ``igui_components`` changes; the view receives the same
//! `&'static dyn Theme` it always did. [`ThemeChoice`] names the built-in and
//! custom themes so the CLI can pick one.

use std::sync::OnceLock;

use igui::igui_core::{Color, FontWeight};
use igui::igui_theme::{
    default_theme, ControlSize, Density, Mode, Palette, Radius, SurfaceLevel, TextSize, Theme,
};

/// Which theme the app should use. `Game` is this crate's custom [`GameTheme`]
/// (the house style, the default); `Default` is the library's built-in theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// `igui::igui_theme::default_theme` — the reference palette.
    Default,
    /// The Classic Game Box house style ([`game_theme`]).
    #[default]
    Game,
}

impl ThemeChoice {
    /// The choices, in the order the settings page lists them.
    pub const ALL: [ThemeChoice; 2] = [ThemeChoice::Game, ThemeChoice::Default];

    /// Parses the `--theme` value or a stored key. `None` for an unknown name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "game" | "custom" => Some(Self::Game),
            _ => None,
        }
    }

    /// Resolves the choice to a `'static` theme for `mode`.
    pub fn theme(self, mode: Mode) -> &'static dyn Theme {
        match self {
            Self::Default => default_theme(mode),
            Self::Game => game_theme(mode),
        }
    }

    /// What the settings page calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::Game => "街机（琥珀）",
        }
    }

    /// The stable key stored in settings.
    pub fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Game => "game",
        }
    }
}

/// The Classic Game Box house style: a warm arcade-amber accent over neutral
/// surfaces, a compact density, restrained 4–8 px radii, bold headings and an
/// opaque selection.
///
/// It owns only the tokens that carry the brand; every other token keeps the
/// [`Theme`](igui::igui_theme::Theme) trait's default, so the theme stays in sync
/// with the library if it adds tokens with sensible defaults.
pub struct GameTheme {
    mode: Mode,
    palette: Palette,
}

impl GameTheme {
    /// Builds the theme for `mode`.
    pub fn new(mode: Mode) -> Self {
        let palette = match mode {
            Mode::Light => Self::light_palette(),
            Mode::Dark => Self::dark_palette(),
        };
        Self { mode, palette }
    }

    fn light_palette() -> Palette {
        let accent = rgba(0xB2, 0x5E, 0x00, 0xFF); // burnt amber, dark enough on paper
        Palette {
            background: rgba(0xF7, 0xF5, 0xF0, 0xFF),
            foreground: rgba(0x1B, 0x1A, 0x17, 0xFF),
            surface: rgba(0xFF, 0xFF, 0xFF, 0xFF),
            surface_raised: rgba(0xF0, 0xED, 0xE6, 0xFF),
            surface_hover: rgba(0xE8, 0xE4, 0xDA, 0xFF),
            muted: rgba(0x6C, 0x68, 0x5F, 0xFF),
            subtle: rgba(0xA6, 0xA1, 0x98, 0xFF),
            border: rgba(0xDC, 0xD7, 0xCC, 0xFF),
            border_subtle: rgba(0xE9, 0xE5, 0xDD, 0xFF),
            code_surface: rgba(0xF1, 0xEE, 0xE7, 0xFF),
            accent,
            success: rgba(0x1E, 0x9E, 0x55, 0xFF),
            warning: rgba(0xB8, 0x79, 0x0A, 0xFF),
            error: rgba(0xCF, 0x3A, 0x2F, 0xFF),
            info: accent,
            on_accent: rgba(0xFF, 0xFF, 0xFF, 0xFF),
            focus_ring: accent.with_alpha(0.45),
            // Solid and opaque: dark text stays readable on the warm fill.
            selection: rgba(0xF5, 0xE4, 0xC6, 0xFF),
        }
    }

    fn dark_palette() -> Palette {
        let accent = rgba(0xFF, 0xB0, 0x20, 0xFF); // arcade amber
        Palette {
            background: rgba(0x0B, 0x0B, 0x0E, 0xFF),
            foreground: rgba(0xF2, 0xF2, 0xF5, 0xFF),
            surface: rgba(0x12, 0x12, 0x16, 0xFF),
            surface_raised: rgba(0x19, 0x19, 0x20, 0xFF),
            surface_hover: rgba(0x23, 0x23, 0x2C, 0xFF),
            muted: rgba(0x9A, 0x9A, 0xA6, 0xFF),
            subtle: rgba(0x6B, 0x6B, 0x78, 0xFF),
            border: rgba(0x2A, 0x2A, 0x35, 0xFF),
            border_subtle: rgba(0x1E, 0x1E, 0x27, 0xFF),
            code_surface: rgba(0x12, 0x12, 0x16, 0xFF),
            accent,
            success: rgba(0x3C, 0xCB, 0x70, 0xFF),
            warning: rgba(0xFF, 0xC5, 0x3D, 0xFF),
            error: rgba(0xFF, 0x5C, 0x52, 0xFF),
            info: accent,
            // Primary / destructive button labels and icons are near-black on
            // the bright amber fill.
            on_accent: rgba(0x16, 0x12, 0x0A, 0xFF),
            focus_ring: accent.with_alpha(0.55),
            selection: accent.with_alpha(0.20),
        }
    }
}

impl Theme for GameTheme {
    fn palette(&self) -> &Palette {
        &self.palette
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn surface(&self, level: SurfaceLevel) -> Color {
        let palette = &self.palette;
        match level {
            SurfaceLevel::Base => palette.background,
            SurfaceLevel::Surface => palette.surface,
            SurfaceLevel::Raised => palette.surface_raised,
            // Overlays are opaque and flat: the same content surface, separated
            // by a 1 px border instead of a frosted/floating layer.
            SurfaceLevel::Floating => palette.surface,
        }
    }

    fn font_weight(&self, size: TextSize) -> FontWeight {
        match size {
            TextSize::Display | TextSize::Title | TextSize::Heading => FontWeight::BOLD,
            _ => FontWeight::NORMAL,
        }
    }

    fn radius(&self, radius: Radius) -> f32 {
        // Restrained geometry: 4–8 px. No large rounded cards, no pills.
        match radius {
            Radius::Sm => 4.0,
            Radius::Md => 6.0,
            Radius::Lg => 8.0,
            Radius::Panel => 8.0,
            Radius::None | Radius::Full => radius.px(),
        }
    }

    fn density(&self) -> Density {
        Density {
            space_scale: 1.0,
            control_height: 28.0,
            control_height_mini: 24.0,
            control_padding_x: 12.0,
            control_padding_y: 8.0,
            row_height: 28.0,
            default_control: ControlSize::Regular,
        }
    }
}

static GAME_LIGHT: OnceLock<GameTheme> = OnceLock::new();
static GAME_DARK: OnceLock<GameTheme> = OnceLock::new();

/// The custom Classic Game Box theme for `mode`, as a `'static` trait object
/// ready to hand to components.
pub fn game_theme(mode: Mode) -> &'static dyn Theme {
    match mode {
        Mode::Light => GAME_LIGHT.get_or_init(|| GameTheme::new(Mode::Light)),
        Mode::Dark => GAME_DARK.get_or_init(|| GameTheme::new(Mode::Dark)),
    }
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_tokens_fall_through_to_the_builtin() {
        let custom = game_theme(Mode::Dark);
        let builtin = default_theme(Mode::Dark);
        // Motion is inherited unchanged (the theme does not override it).
        assert_eq!(custom.motion_fast(), builtin.motion_fast());
    }

    #[test]
    fn the_house_style_overrides_the_brand_tokens() {
        let custom = game_theme(Mode::Dark);
        let builtin = default_theme(Mode::Dark);
        assert_ne!(custom.palette().accent, builtin.palette().accent);
        assert_eq!(custom.palette().accent.to_rgba8(), [0xFF, 0xB0, 0x20, 0xFF]);
        // Overlays are opaque and flat, not the raised surface.
        assert_eq!(
            custom.surface(SurfaceLevel::Floating),
            custom.palette().surface
        );
        assert_eq!(custom.font_weight(TextSize::Heading), FontWeight::BOLD);
    }

    #[test]
    fn theme_choice_parses_and_resolves() {
        assert_eq!(ThemeChoice::parse("default"), Some(ThemeChoice::Default));
        assert_eq!(ThemeChoice::parse("game"), Some(ThemeChoice::Game));
        assert_eq!(ThemeChoice::parse("custom"), Some(ThemeChoice::Game));
        assert_eq!(ThemeChoice::parse("nope"), None);
        assert_eq!(ThemeChoice::default(), ThemeChoice::Game);
        for choice in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::parse(choice.key()), Some(choice));
        }
        assert_eq!(
            ThemeChoice::Game.theme(Mode::Light).palette().accent,
            GameTheme::new(Mode::Light).palette().accent
        );
    }
}
