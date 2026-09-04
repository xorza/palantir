//! What a spinner wears, and how fast it turns.

use crate::primitives::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals and motion for [`crate::Spinner`]: the rotating comet arc.
/// Builder overrides (`.color(...)` / `.diameter(...)` /
/// `.thickness(...)`) win; otherwise these fill in.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    /// Arc color — the comet's head; the tail fades to transparent.
    pub color: RgbaF32,
    /// Diameter in logical px.
    pub diameter: f32,
    /// Arc length in radians. Under a full turn, so the gap is what
    /// reads as motion; `TAU` would look like a static ring.
    pub sweep: f32,
    /// Rotation rate in radians/second.
    pub speed: f32,
    /// Stroke width as a fraction of the diameter, so a resized spinner
    /// keeps its proportions instead of thinning out.
    pub thickness_ratio: f32,
    /// Floor on the derived stroke width, in logical px — a tiny
    /// spinner still needs a visible arc.
    pub min_thickness: f32,
}

impl SpinnerTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.accent,
            diameter: 24.0,
            sweep: 1.5 * std::f32::consts::PI,
            speed: 4.5,
            thickness_ratio: 0.12,
            min_thickness: 1.5,
        }
    }
}

palette_default!(SpinnerTheme);
