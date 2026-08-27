//! What a divider rule wears. Its default margin depends on where the
//! rule is used, so a menu's separator names its own.

use crate::primitives::color::Color;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Separator`]: the thin divider rule between
/// content. Builder overrides (`.color(...)` / `.thickness(...)` /
/// `.margin(...)`) win; otherwise these defaults fill in, so a
/// design-system theme restyles separators the same way it restyles
/// every other widget.
///
/// Also the bundle behind [`crate::MenuSeparator`], through
/// [`crate::ContextMenuTheme::separator`]. A menu rule is the same
/// object as an in-flow one wearing different values — it just spans a
/// padded popup rather than a content column, which is what `margin`
/// expresses — so the two share a type rather than the menu keeping a
/// near-duplicate of this one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeparatorTheme {
    /// Rule color.
    pub color: Color,
    /// Rule breadth in logical px.
    pub thickness: f32,
    /// Breathing room around the rule, applied when the builder left
    /// margin unset. `ZERO` for an in-flow rule; the menu slot opens a
    /// vertical gutter instead — horizontal inset would leave a menu
    /// rule visibly short of the labels it divides, since it already
    /// spans only the panel's padded width.
    pub margin: Spacing,
}

impl SeparatorTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.border_soft(),
            thickness: 1.0,
            margin: Spacing::ZERO,
        }
    }

    /// The [`crate::MenuSeparator`] recipe: the same rule, held off the
    /// rows above and below it.
    pub fn menu_separator(p: &Palette) -> Self {
        Self {
            margin: Spacing::xy(0.0, 4.0),
            ..Self::from_palette(p)
        }
    }
}

palette_default!(SeparatorTheme);
