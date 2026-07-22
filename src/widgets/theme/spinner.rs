use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Spinner`]: the rotating comet arc. Builder
/// overrides (`.color(...)` / `.diameter(...)`) win; otherwise these
/// defaults fill in. Stroke thickness stays diameter-derived on the widget
/// (`diameter * 0.12`, floored) unless overridden per call.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    /// Arc color — the comet's head; the tail fades to transparent.
    pub color: Color,
    /// Diameter in logical px.
    pub diameter: f32,
}

impl SpinnerTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.accent,
            diameter: 24.0,
        }
    }
}

impl Default for SpinnerTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
