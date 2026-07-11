use crate::primitives::color::Color;
use crate::widgets::theme::palette;

/// Visuals for [`crate::Splitter`]: the divider bar between the two
/// panes. The bar is `thickness` across — the whole breadth is the drag
/// target — but at rest only a centered `rule_thickness` line paints in
/// `rule`; the full bar fills with `hover` under the pointer and `drag`
/// while a resize is in flight.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SplitterTheme {
    /// Divider bar breadth in logical px — the draggable hit area.
    pub thickness: f32,
    /// Resting rule color (the thin center line).
    pub rule: Color,
    /// Resting rule breadth in logical px.
    pub rule_thickness: f32,
    /// Full-bar fill while hovered.
    pub hover: Color,
    /// Full-bar fill while dragging.
    pub drag: Color,
}

impl Default for SplitterTheme {
    fn default() -> Self {
        Self {
            thickness: 6.0,
            rule: palette::TEXT_MUTED.with_alpha(0.18),
            rule_thickness: 1.0,
            hover: palette::ELEM_HOVER,
            drag: palette::ACCENT.with_alpha(0.6),
        }
    }
}
