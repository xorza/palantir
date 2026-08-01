use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::MenuSeparator`], the divider between menu
/// groups. Its own bundle rather than a reach into
/// [`crate::Theme::separator`]: a menu rule is a different object from
/// an in-flow one — it spans a 4 px-padded popup, not a content column,
/// and restyling the menu shouldn't have to drag every other rule in
/// the app along with it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuSeparatorTheme {
    /// Rule color.
    pub color: Color,
    /// Rule breadth in logical px.
    pub thickness: f32,
    /// Breathing room around the rule. Vertical only by default — the
    /// rule spans the full padded width, so a horizontal inset would
    /// leave it visibly short of the labels it divides.
    pub margin: Spacing,
}

impl MenuSeparatorTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.border_soft(),
            thickness: 1.0,
            margin: Spacing::xy(0.0, 4.0),
        }
    }
}

palette_default!(MenuSeparatorTheme);
