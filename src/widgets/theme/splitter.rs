use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Splitter`]: the divider between the two panes.
/// Layout reserves only the `rule_thickness` seam (painted in `rule`);
/// the `thickness`-wide drag target is an overlay straddling the seam,
/// invisible at rest, filling with `hover` under the pointer and `drag`
/// while a resize is in flight (covering the pane edges beneath it).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SplitterTheme {
    /// Overlay grab-bar breadth in logical px — the draggable hit area.
    pub thickness: f32,
    /// Resting rule color (the visible seam between the panes).
    pub rule: Color,
    /// Rule breadth in logical px — the layout space the seam reserves.
    pub rule_thickness: f32,
    /// Full-bar fill while hovered.
    pub hover: Color,
    /// Full-bar fill while dragging.
    pub drag: Color,
}

impl SplitterTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            thickness: 6.0,
            rule: p.border_soft(),
            rule_thickness: 1.0,
            hover: p.elem_hover,
            drag: p.accent.with_alpha(0.6),
        }
    }
}

impl Default for SplitterTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
