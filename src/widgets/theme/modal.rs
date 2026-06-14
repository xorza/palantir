use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;

/// Visuals for [`crate::widgets::modal::Modal`]: the centered dialog
/// card plus the dimming backdrop behind it. Builder overrides
/// (`.background(...)` / `.backdrop(...)`) win; otherwise these defaults
/// fill in, so an app's design-system theme can restyle modals the same
/// way it restyles tooltips and context menus.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModalTheme {
    /// Dialog card chrome (fill + stroke + radius + optional shadow).
    pub card: Background,
    /// Dimming scrim painted behind the card. Straight-alpha linear —
    /// black at partial alpha reads as a neutral dim.
    pub backdrop: Color,
    /// Padding inside the card, applied when the builder leaves it unset.
    pub padding: Spacing,
    /// Minimum card width in logical px (the card hugs its content above
    /// this floor).
    pub min_width: f32,
}

impl Default for ModalTheme {
    fn default() -> Self {
        let card = Background {
            fill: palette::ELEM_HOVER.into(),
            stroke: Stroke::solid(palette::TEXT_MUTED.with_alpha(0.25), 1.0),
            corners: Corners::all(12.0),
            shadow: Shadow::NONE,
        };
        Self {
            card,
            // Straight-alpha linear black at 50% — a dim scrim. Black is
            // identical in sRGB and linear, so `linear_rgba` is exact.
            backdrop: Color::linear_rgba(0.0, 0.0, 0.0, 0.5),
            padding: Spacing::all(20.0),
            min_width: 280.0,
        }
    }
}
