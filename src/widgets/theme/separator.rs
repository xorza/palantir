use crate::primitives::color::Color;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Separator`]: the thin divider rule between
/// content. Builder overrides (`.color(...)` / `.thickness(...)`) win;
/// otherwise these defaults fill in, so a design-system theme restyles
/// separators the same way it restyles every other widget.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeparatorTheme {
    /// Rule color.
    pub color: Color,
    /// Rule breadth in logical px.
    pub thickness: f32,
}

impl SeparatorTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.border_soft(),
            thickness: 1.0,
        }
    }
}

impl Default for SeparatorTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
