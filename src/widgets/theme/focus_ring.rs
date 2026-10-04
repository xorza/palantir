//! The ring the framework draws around the widget that holds keyboard
//! focus.

use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// The ring around the focused widget, drawn by the framework rather than
/// by any widget, and only while focus came from the keyboard — the rule
/// CSS states as `:focus-visible`.
///
/// It paints with the widget's chrome, inside the widget's arranged rect
/// and with its chrome's corners, so the widget's own clip never cuts it
/// and the layout never moves to make room for it.
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
