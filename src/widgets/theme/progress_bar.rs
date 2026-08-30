//! What a progress bar wears: the track it runs along, and the fill
//! that measures the fraction.

use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::ProgressBar`]: a rounded `track` with an accent
/// `fill` spanning the value. The pill corner radius is
/// `thickness / 2`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProgressBarTheme {
    /// Track color behind the fill.
    pub track: Color,
    /// Fill color (the completed portion).
    pub fill: Color,
    /// Cross-axis thickness of the bar in logical px.
    pub thickness: f32,
}

impl ProgressBarTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            track: p.elem_mid,
            fill: p.accent,
            thickness: 6.0,
        }
    }
}

palette_default!(ProgressBarTheme);
