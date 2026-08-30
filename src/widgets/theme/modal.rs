//! What a modal wears: the dialog surface, and the backdrop that dims
//! everything behind it.

use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::widgets::modal::Modal`]: the centered dialog
/// panel plus the dimming backdrop behind it. Builder overrides
/// (`.background(...)` / `.backdrop(...)`) win; otherwise these defaults
/// fill in, so an app's design-system theme can restyle modals the same
/// way it restyles tooltips and context menus.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModalTheme {
    /// Dialog panel chrome (fill + stroke + radius + optional shadow).
    pub panel: Background,
    /// Dimming scrim painted behind the panel. Straight-alpha linear —
    /// black at partial alpha reads as a neutral dim.
    pub backdrop: Color,
    /// Padding inside the panel, applied when the builder leaves it unset.
    pub padding: Spacing,
    /// Minimum panel width in logical px (the panel hugs its content
    /// above this floor).
    pub min_width: f32,
}

impl ModalTheme {
    pub fn from_palette(p: &Palette) -> Self {
        let panel = Background::rounded(p.elem_mid, Corners::all(12.0))
            .with_stroke(Stroke::solid(p.border_mid(), 1.0));
        Self {
            panel,
            // Straight-alpha linear black at 50% — a dim scrim. Black is
            // identical in sRGB and linear, so `linear_rgba` is exact.
            backdrop: Color::linear_rgba(0.0, 0.0, 0.0, 0.5),
            padding: Spacing::all(20.0),
            min_width: 280.0,
        }
    }
}

palette_default!(ModalTheme);
