//! What a divider rule wears. The default margin depends on use, so a menu separator names its own.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Separator`]. Builder overrides (`.color`, `.thickness`, `.margin`) win; these fill in otherwise.
///
/// Also the bundle for [`crate::MenuSeparator`] via [`crate::ContextMenuTheme::separator`]: the same rule with different values.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeparatorTheme {
    /// Rule color.
    pub color: RgbaF32,
    /// Rule breadth in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub thickness: f32,
    /// Breathing room when the builder left margin unset. `ZERO` in flow; the menu slot uses a vertical gutter, since a horizontal inset would leave the rule short of the labels.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::margin")]
    pub margin: Spacing,
}

impl SeparatorTheme {
    /// A one-pixel rule in the palette's softest border colour, no margin.
    pub const fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.border_soft(),
            thickness: 1.0,
            margin: Spacing::ZERO,
        }
    }

    /// The [`crate::MenuSeparator`] recipe: the same rule, held off the rows around it.
    pub fn menu_separator(p: &Palette) -> Self {
        Self {
            margin: Spacing::xy(0.0, 4.0),
            ..Self::from_palette(p)
        }
    }
}

impl Default for SeparatorTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
