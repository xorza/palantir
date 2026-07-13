use crate::primitives::color::Color;
use crate::widgets::theme::palette;

/// Visuals for [`crate::Separator`]: the thin divider rule between
/// content. Builder overrides (`.color(...)` / `.thickness(...)`) win;
/// otherwise these defaults fill in, so a design-system theme restyles
/// separators the same way it restyles every other widget.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeparatorTheme {
    /// Rule color.
    pub color: Color,
    /// Rule breadth in logical px.
    pub thickness: f32,
}

impl Default for SeparatorTheme {
    fn default() -> Self {
        Self {
            color: palette::TEXT_MUTED.with_alpha(0.18),
            thickness: 1.0,
        }
    }
}
