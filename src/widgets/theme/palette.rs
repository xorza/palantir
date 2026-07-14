//! The color roster every theme recipe draws from. [`Palette`] is the
//! public input to [`crate::Theme::from_palette`] — apps hand in their
//! own swatches and every widget recolors from one source instead of
//! re-deriving aperture's recipes per widget. [`Palette::DEFAULT`] is
//! the built-in neutral dark grayscale with a single blue accent.

use crate::primitives::color::Color;

/// Semantic color roster for theme assembly. Fields are the roles the
/// widget recipes key on; derived tints (the border ladder) live as
/// methods so a palette swap moves them automatically.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    /// Primary foreground / label ink.
    pub text: Color,
    /// De-emphasized foreground; also the base of the border ladder.
    pub text_muted: Color,
    /// Disabled-state foreground.
    pub text_disabled: Color,
    /// Window / editor background (`Theme::window_clear`).
    pub terminal_bg: Color,
    /// Resting surface tier (disabled fills, menu panels).
    pub elem: Color,
    /// One step brighter — resting chrome for interactive surfaces.
    pub elem_hover: Color,
    /// Two steps brighter — hover/press emphasis tier.
    pub elem_active: Color,
    /// Focus-ring / pressed-stroke color.
    pub border_focused: Color,
    /// The accent (checked toggles, progress fill, selection wash).
    pub accent: Color,
}

impl Palette {
    /// Built-in neutral dark palette — the values `Theme::default`
    /// assembles from.
    pub const DEFAULT: Self = Self {
        text: Color::hex(0xffffff),
        text_muted: Color::hex(0xaaaaa8),
        text_disabled: Color::hex(0x878a8d),
        terminal_bg: Color::hex(0x1a1a1a),
        elem: Color::hex(0x343434),
        elem_hover: Color::hex(0x3e3e3e),
        elem_active: Color::hex(0x4b4b4b),
        border_focused: Color::hex(0x105577),
        accent: Color::hex(0x9adbfb),
    };

    // The border ladder — TEXT_MUTED tints, not grays: raw surface
    // grays sit too close to `elem`/`elem_hover` to read as edges at
    // 1 px.
    pub fn border_soft(&self) -> Color {
        self.text_muted.with_alpha(0.18)
    }

    pub fn border_mid(&self) -> Color {
        self.text_muted.with_alpha(0.22)
    }

    pub fn border_strong(&self) -> Color {
        self.text_muted.with_alpha(0.35)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}
