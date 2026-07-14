use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Slider`]: a thin two-tone rail (filled `fill`
/// left of the knob, `rail` right of it) with a round `knob`. The rail
/// is `rail_thickness` tall and pill-capped; the knob is `knob_size`
/// across.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SliderTheme {
    /// Unfilled rail color (right of the knob).
    pub rail: Color,
    /// Filled rail color (left of the knob).
    pub fill: Color,
    /// Knob (handle) color.
    pub knob: Color,
    /// Knob diameter in logical px — also the widget's height.
    pub knob_size: f32,
    /// Rail thickness in logical px. Pill radius is `rail_thickness / 2`.
    pub rail_thickness: f32,
}

impl SliderTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            rail: p.elem_hover,
            fill: p.accent,
            knob: p.text,
            knob_size: 18.0,
            rail_thickness: 4.0,
        }
    }
}

impl Default for SliderTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
