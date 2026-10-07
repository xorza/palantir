//! What a modal wears: the dialog surface and the dimming backdrop.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::widgets::modal::Modal`]. Builder overrides (`.background(...)` / `.backdrop(...)`) win; these defaults fill in.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModalTheme {
    /// Dialog panel chrome.
    pub panel: Background,
    /// Dimming scrim; straight-alpha linear.
    pub backdrop: RgbaF32,
    /// Padding inside the panel, applied when the builder leaves it unset.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Minimum panel width in logical px; the panel hugs its content above it.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
}

impl ModalTheme {
    /// A raised panel over a half-opaque black scrim.
    pub fn from_palette(p: &Palette) -> Self {
        let panel = Background::rounded(p.element_mid, Corners::all(12.0))
            .with_border(Stroke::new(p.border_mid(), 1.0));
        Self {
            panel,
            // Black is identical in sRGB and linear, so `RgbaF32::new` is exact.
            backdrop: RgbaF32::new(0.0, 0.0, 0.0, 0.5),
            padding: Spacing::all(20.0),
            min_width: 280.0,
        }
    }
}

impl Default for ModalTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
