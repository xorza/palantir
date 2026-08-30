//! What a splitter's divider wears, and how wide it is to grab.

use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Splitter`]: the divider between the two panes.
/// Layout reserves only the `rule_thickness` seam (painted in `rule`);
/// the `grab_thickness`-wide drag target is an overlay straddling the
/// seam, invisible at rest, filling with `hovered` under the pointer
/// and `active` while a resize is in flight (covering the pane edges
/// beneath it).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SplitterTheme {
    /// Overlay grab-bar breadth in logical px — the draggable hit area.
    pub grab_thickness: f32,
    /// Resting rule color (the visible seam between the panes).
    pub rule: Color,
    /// Rule breadth in logical px — the layout space the seam reserves.
    pub rule_thickness: f32,
    /// Full-bar fill while hovered.
    pub hovered: Color,
    /// Full-bar fill while a resize drag is in flight.
    pub active: Color,
}

impl SplitterTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            grab_thickness: 6.0,
            rule: p.border_soft(),
            rule_thickness: 1.0,
            hovered: p.elem_mid,
            active: p.accent.with_alpha(0.6),
        }
    }
}

palette_default!(SplitterTheme);
