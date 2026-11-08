//! What a divider rule wears. The default margin depends on use, so a menu separator names its own.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::Separator`]. Builder overrides (`.color`, `.thickness`, `.margin`) win; these fill in otherwise.
///
/// Also the bundle for [`crate::MenuSeparator`] via [`crate::MenuTheme::separator`]: the same rule with different values.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct SeparatorTheme {
    /// Rule color.
    pub color: RgbaF32,
    /// Rule breadth in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub thickness: f32,
    /// Breathing room when the builder left margin unset. `ZERO` in flow; the menu slot uses a vertical gutter, since a horizontal inset would leave the rule short of the labels.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::margin")]
    pub margin: Spacing,
}

impl SeparatorTheme {
    /// A hairline rule in the palette's softest border color, no margin.
    pub const fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            color: p.border_soft(),
            thickness: m.stroke.hairline,
            margin: Spacing::ZERO,
        }
    }

    /// The [`crate::MenuSeparator`] recipe: the same rule, held off the rows around it.
    pub fn menu(p: &Palette, m: &Metrics) -> Self {
        Self {
            margin: Spacing::xy(0.0, m.space.xs),
            ..Self::from_parts(p, m)
        }
    }
}
