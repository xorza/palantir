//! What a spinner wears, and how fast it turns.

use crate::primitives::paint::color::RgbaF32;
use crate::theme::palette::Palette;
use std::f32::consts;
use std::time::Duration;

/// Visuals and motion for [`crate::Spinner`]. Builder overrides (`.color(...)` / `.diameter(...)` / `.width(...)`) win.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    /// Arc color at the comet's head; the tail fades to transparent.
    pub color: RgbaF32,
    /// Diameter in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub diameter: f32,
    /// Arc length in radians; under a full turn, or it reads as a static ring.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub sweep: f32,
    /// One full turn. A zero period holds the arc still.
    #[serde(with = "crate::theme::duration_codec::period")]
    pub period: Duration,
    /// Stroke width as a fraction of the diameter.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub width_ratio: f32,
    /// Floor on the derived stroke width in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
}

impl SpinnerTheme {
    /// An accent arc sweeping three quarters of the circle.
    pub const fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.accent,
            diameter: 24.0,
            sweep: 1.5 * consts::PI,
            period: Duration::from_millis(1400),
            width_ratio: 0.12,
            min_width: 1.5,
        }
    }
}

impl Default for SpinnerTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
