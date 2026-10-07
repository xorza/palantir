//! The ring drawn around the keyboard-focused widget.

use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// The ring around the focused widget, drawn by the framework, only while focus came from the keyboard (CSS `:focus-visible`).
///
/// It paints inside the widget's arranged rect with its chrome's corners, so the widget's clip never cuts it and layout never moves.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FocusRingTheme {
    /// Ring colour.
    pub color: RgbaF32,
    /// Ring width in logical px. `0` draws no ring.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub width: f32,
}

impl FocusRingTheme {
    /// A two-pixel ring in the palette's accent.
    pub const fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.accent,
            width: 2.0,
        }
    }
}

impl Default for FocusRingTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
