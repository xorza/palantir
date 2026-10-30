//! What a tooltip wears, and how long a hover must last before it appears.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::ThemeText;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use std::time::Duration;

/// Visuals and timing for [`crate::widgets::tooltip::Tooltip`]. Bubbles paint into `Layer::Tooltip` after `delay` of hover; the `warmup` window keeps later tooltips instant after one is dismissed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TooltipTheme {
    /// Bubble chrome (fill + stroke + radius + optional shadow).
    pub panel: Chrome,
    /// Text axes the bubble sets over [`Theme::text`](crate::Theme).
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub text: TextStyleOverrides,
    /// Padding between chrome and the text.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Cap on the bubble's outer size. Width gates wrap; height is usually `INF`. Override via `.max_size(...)`. Infinite axes round-trip because `Size`'s serde maps non-finite to `None`.
    pub max_size: Size,
    /// How long the pointer must rest on the trigger before the bubble shows (cold start); seconds in a file.
    #[serde(with = "crate::theme::duration_codec::seconds")]
    pub delay: Duration,
    /// How long after a dismissal the next tooltip shows at once; zero disables it. Seconds in a file.
    #[serde(with = "crate::theme::duration_codec::seconds")]
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

    /// A small control-rounded bubble that floats as a menu does.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let panel = Chrome::rounded(p.element, Corners::all(m.radius.control))
            .with_border(Stroke::new(p.border_mid(), m.stroke.hairline))
            .with_shadow(m.float_shadow);
        Self {
            panel,
            // The caption tier, a step under the 14 px body, as Fluent's tooltip sets its text.
            text: TextStyleOverrides::NONE.with_font_size(12.0),
            // Fluent's tooltip insets its text 12 px on each side.
            padding: Spacing::xy(m.space.md, m.space.xs),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: Duration::from_millis(500),
            warmup: Duration::from_secs(1),
            gap: m.space.sm,
        }
    }
}

impl Default for TooltipTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DEFAULT, &Metrics::DEFAULT)
    }
}
