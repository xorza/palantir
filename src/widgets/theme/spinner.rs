//! What a spinner wears, and how fast it turns.

use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;
use std::f32::consts;

/// Visuals and motion for [`crate::Spinner`]. Builder overrides (`.color(...)` / `.diameter(...)` / `.thickness(...)`) win.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    /// Arc colour at the comet's head; the tail fades to transparent.
    pub color: RgbaF32,
    /// Diameter in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub diameter: f32,
    /// Arc length in radians; under a full turn, or it reads as a static ring.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub sweep: f32,
    /// Rotation rate in radians/second.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::positive")]
    pub speed: f32,
    /// Stroke width as a fraction of the diameter.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub thickness_ratio: f32,
    /// Floor on the derived stroke width in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_thickness: f32,
}

impl SpinnerTheme {
    /// An accent arc sweeping three quarters of the circle.
    pub const fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.accent,
            diameter: 24.0,
            sweep: 1.5 * consts::PI,
            speed: 4.5,
            thickness_ratio: 0.12,
            min_thickness: 1.5,
        }
    }
}

impl Default for SpinnerTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
