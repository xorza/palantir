//! What a splitter's divider wears, and how wide it is to grab.

use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Splitter`]: layout reserves the `rule_thickness` seam (painted in `rule`); the `grab_thickness` drag target is an invisible overlay straddling it, filled `hovered` or `active`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SplitterTheme {
    /// Grab-bar breadth in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub grab_thickness: f32,
    /// Resting rule colour.
    pub rule: RgbaF32,
    /// Rule breadth in logical px; the layout space reserved.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub rule_thickness: f32,
    /// Full-bar fill while hovered.
    pub hovered: RgbaF32,
    /// Full-bar fill while a resize drag is in flight.
    pub active: RgbaF32,
}

impl SplitterTheme {
    /// A hairline rule in a six-pixel grab band.
    pub const fn from_palette(p: &Palette) -> Self {
        Self {
            grab_thickness: 6.0,
            rule: p.border_soft(),
            rule_thickness: 1.0,
            hovered: p.element_mid,
            active: p.accent.with_alpha(0.6),
        }
    }
}

impl Default for SplitterTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
