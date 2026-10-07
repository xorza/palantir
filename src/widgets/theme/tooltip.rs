//! What a tooltip wears, and how long a hover must last before it appears.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;
use glam::Vec2;
use std::time::Duration;

/// Visuals and timing for [`crate::widgets::tooltip::Tooltip`]. Bubbles paint into `Layer::Tooltip` after `delay` of hover; the `warmup` window keeps later tooltips instant after one is dismissed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TooltipTheme {
    /// Bubble chrome (fill + stroke + radius + optional shadow).
    pub panel: Background,
    /// Text axes the bubble sets over [`Theme::text`](crate::Theme).
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub text: TextStyleOverrides,
    /// Padding between chrome and the text.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Cap on the bubble's outer size. Width gates wrap; height is usually `INF`. Override via `.max_size(...)`. Infinite axes round-trip because `Size`'s serde maps non-finite to `None`.
    pub max_size: Size,
    /// Seconds the pointer must rest on the trigger before the bubble shows (cold start).
    #[serde(with = "crate::widgets::theme::serde::duration_seconds")]
    pub delay: Duration,
    /// Seconds after a dismissal during which the next tooltip is instant; 0 disables.
    #[serde(with = "crate::widgets::theme::serde::duration_seconds")]
    pub warmup: Duration,
    /// Gap in logical px between trigger rect and bubble.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub gap: f32,
}

impl TooltipTheme {
    /// Visit every text slot this theme owns, driving `Theme::scale_text`. Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            text,
            panel: _,
            padding: _,
            max_size: _,
            delay: _,
            warmup: _,
            gap: _,
        } = self;
        f(ThemeText::Overrides(text));
    }

    /// A small raised bubble with a soft drop shadow.
    pub fn from_palette(p: &Palette) -> Self {
        let panel = Background::rounded(p.element, Corners::all(4.0))
            .with_border(Stroke::new(p.border_mid(), 1.0))
            .with_shadow(Shadow::drop(
                RgbaF32::new(0.0, 0.0, 0.0, 0.6),
                Vec2::new(2.0, 2.0),
                5.0,
            ));
        Self {
            panel,
            text: TextStyleOverrides::NONE.with_font_size(13.0),
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
