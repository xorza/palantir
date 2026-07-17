use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use glam::Vec2;
use std::time::Duration;

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
    /// callers override via `.max_size(...)` (`Configure`). The
    /// infinite height axis round-trips because `Size`'s serde maps
    /// non-finite axes to `None`.
    pub max_size: Size,
    /// Seconds the pointer must rest on the trigger before the bubble
    /// shows (cold start).
    #[serde(with = "duration_seconds")]
    pub delay: Duration,
    /// Seconds after a tooltip is dismissed during which the next
    /// tooltip appears instantly (warmup). Set to 0 to disable.
    #[serde(with = "duration_seconds")]
    pub warmup: Duration,
    /// Gap in logical px between trigger rect and bubble.
    pub gap: f32,
}

impl TooltipTheme {
    /// Visit every `TextStyle` this theme owns — drives `Theme::set_text_scale`.
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        f(&mut self.text);
    }
}

impl TooltipTheme {
    pub fn from_palette(p: &Palette) -> Self {
        let panel = Background::rounded(p.elem, Corners::all(4.0))
            .with_stroke(Stroke::solid(p.border_mid(), 1.0))
            .with_shadow(Shadow::drop(
                Color::linear_rgba(0.0, 0.0, 0.0, 0.6),
                Vec2::new(2.0, 2.0),
                5.0,
            ));
        Self {
            panel,
            text: TextStyle::default().with_font_size(13.0).with_color(p.text),
            padding: Spacing::xy(6.0, 4.0),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: Duration::from_millis(500),
            warmup: Duration::from_secs(1),
            gap: 6.0,
        }
    }
}

impl Default for TooltipTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

mod duration_seconds {
    use serde::de::Error as _;
    use std::time::Duration;

    const ERROR: &str = "tooltip timing must be finite, non-negative, and representable";

    pub(crate) fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f32(duration.as_secs_f32())
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let secs = <f32 as serde::Deserialize>::deserialize(deserializer)?;
        Duration::try_from_secs_f32(secs).map_err(|_| D::Error::custom(ERROR))
    }
}
