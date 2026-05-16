use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::text_style::TextStyle;

/// Visuals + timing for [`crate::widgets::tooltip::Tooltip`]. Bubbles
/// paint into `Layer::Tooltip` after the pointer has hovered a trigger
/// for `delay` seconds; the `warmup` window keeps subsequent tooltips
/// instant for a short period after one was dismissed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TooltipTheme {
    /// Bubble chrome (fill + stroke + radius + optional shadow).
    pub panel: Background,
    /// Text inside the bubble.
    pub text: TextStyle,
    /// Padding between chrome and the text.
    pub padding: Spacing,
    /// Cap on the bubble's outer size. Width gates wrap; height is
    /// usually `INF` so tall tooltips just keep growing. Builder
    /// callers override via `.max_size(...)` (`Configure`).
    pub max_size: Size,
    /// Seconds the pointer must rest on the trigger before the bubble
    /// shows (cold start).
    pub delay: f32,
    /// Seconds after a tooltip is dismissed during which the next
    /// tooltip appears instantly (warmup). Set to 0 to disable.
    pub warmup: f32,
    /// Gap in logical px between trigger rect and bubble.
    pub gap: f32,
}

impl Default for TooltipTheme {
    fn default() -> Self {
        let m = palette::TEXT_MUTED;
        let edge = m.with_alpha(0.22);
        let panel = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius: Corners::all(4.0),
            shadow: Shadow {
                color: Color::linear_rgba(0.0, 0.0, 0.0, 0.6),
                offset: glam::Vec2::new(2.0, 2.0),
                blur: 5.0,
                spread: 0.0,
                inset: false,
            },
        };
        Self {
            panel,
            text: TextStyle::default().with_font_size(13.0),
            padding: Spacing::xy(6.0, 4.0),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: 0.5,
            warmup: 1.0,
            gap: 6.0,
        }
    }
}
