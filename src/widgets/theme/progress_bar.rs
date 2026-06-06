use crate::primitives::color::Color;
use crate::widgets::theme::palette;

/// Visuals for [`crate::ProgressBar`]: a rounded `track` rail with an
/// accent `fill` spanning the value. `height` is the bar thickness; the
/// pill corner radius is `height / 2`.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProgressBarTheme {
    /// Rail color behind the fill.
    pub track: Color,
    /// Fill color (the completed portion).
    pub fill: Color,
    /// Bar thickness in logical px. The pill radius is `height / 2`.
    pub height: f32,
}

impl Default for ProgressBarTheme {
    fn default() -> Self {
        Self {
            track: palette::ELEM_HOVER,
            fill: palette::ACCENT,
            height: 6.0,
        }
    }
}
