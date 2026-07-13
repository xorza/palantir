use crate::primitives::color::Color;
use crate::widgets::theme::palette;

/// Visuals for [`crate::Spinner`]: the rotating comet arc. Builder
/// overrides (`.color(...)` / `.size(...)`) win; otherwise these
/// defaults fill in. Stroke thickness stays size-derived on the widget
/// (`size * 0.12`, floored) unless overridden per call.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    /// Arc color — the comet's head; the tail fades to transparent.
    pub color: Color,
    /// Diameter in logical px.
    pub size: f32,
}

impl Default for SpinnerTheme {
    fn default() -> Self {
        Self {
            color: palette::ACCENT,
            size: 24.0,
        }
    }
}
