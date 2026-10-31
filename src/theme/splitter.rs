//! What a splitter's divider wears, and how wide it is to grab.

use crate::primitives::paint::color::RgbaF32;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::Splitter`]: layout reserves the `rule_thickness` seam (painted in `rule`); the `grab_thickness` drag target is an invisible overlay straddling it, filled `hovered` or `active`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SplitterTheme {
    /// Grab-bar breadth in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub grab_thickness: f32,
    /// Resting rule color.
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
    pub const fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            grab_thickness: 6.0,
            rule: p.border_soft(),
            rule_thickness: m.stroke.hairline,
            hovered: p.element_mid,
            active: p.accent.with_alpha8(153),
        }
    }
}

impl Default for SplitterTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
