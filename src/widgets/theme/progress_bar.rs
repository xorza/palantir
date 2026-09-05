//! What a progress bar wears: the track it runs along, and the fill
//! that measures the fraction.

use crate::primitives::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::ProgressBar`]: a rounded `track` with an accent
/// `fill` spanning the value. The pill corner radius is
/// `thickness / 2`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProgressBarTheme {
    /// Track color behind the fill.
    pub track: RgbaF32,
    /// Fill color (the completed portion).
    pub fill: RgbaF32,
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

impl Default for ProgressBarTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
