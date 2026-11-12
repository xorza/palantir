//! What a tooltip wears, and how long a hover must last before it appears.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::material::Material;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use std::time::Duration;

/// Visuals and timing for [`crate::widgets::tooltip::Tooltip`]. Bubbles paint into `Layer::Tooltip` after `delay` of hover; the `warmup` window keeps later tooltips instant after one is dismissed.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct TooltipTheme {
    /// Bubble chrome (fill + stroke + radius + optional shadow).
    pub panel: Chrome,
    /// The material the bubble wears under its fill, drawn as a shape under it
    /// ([`MaterialPaint`](crate::MaterialPaint)); `None` keeps the bubble opaque, which a file that
    /// leaves it out reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// Text axes the bubble sets over [`Theme::text`](crate::Theme).
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub text: TextStyleOverrides,
    /// Padding between chrome and the text.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub padding: Spacing,
    /// Cap on the bubble's outer size. Width gates wrap; height is usually `INF`. Override via `.max_size(...)`. Infinite axes round-trip because `Size`'s serde maps non-finite to `None`.
    pub max_size: Size,
    /// How long the pointer must rest on the trigger before the bubble shows (cold start); seconds in a file.
    #[serde(with = "crate::theme::duration_codec::seconds")]
    #[animate(snap)]
    pub delay: Duration,
    /// How long after a dismissal the next tooltip shows at once; zero disables it. Seconds in a file.
    #[serde(with = "crate::theme::duration_codec::seconds")]
    #[animate(snap)]
    pub warmup: Duration,
    /// Gap in logical px between trigger rect and bubble.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
}

impl TooltipTheme {
    /// A small control-rounded bubble that floats as a menu does, in [`Metrics::float_material`]
    /// held for its text's ink.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let mut panel = Chrome::rounded(p.element, Corners::all(m.radius.control))
            .with_border(Border::new(p.border_mid(), m.stroke.hairline))
            .with_shadow(p.float_shadow(m));
        let mut material = None;
        m.dress_float(&mut panel, &mut material, p.text);
        Self {
            panel,
            material,
            // The caption tier, a step under the body, as Fluent's tooltip sets its text.
            text: TextStyleOverrides::NONE.with_font_size(m.text.caption),
            // Fluent's tooltip insets its text 12 px on each side.
            padding: Spacing::xy(m.space.md, m.space.xs),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: Duration::from_millis(500),
            warmup: Duration::from_secs(1),
            gap: m.space.sm,
        }
    }
}
