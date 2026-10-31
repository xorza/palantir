//! What a modal wears: the dialog surface and the dimming backdrop.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::widgets::modal::Modal`]. Builder overrides (`.chrome(...)` / `.backdrop(...)`) win; these defaults fill in.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModalTheme {
    /// Dialog panel chrome.
    pub panel: Chrome,
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
    /// A floating panel over a half-opaque black scrim.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let panel = Chrome::rounded(p.element_mid, Corners::all(m.radius.float))
            .with_border(Stroke::new(p.border_mid(), m.stroke.hairline))
            .with_shadow(p.float_shadow(m));
        Self {
            panel,
            // Black is identical in sRGB and linear, so `RgbaF32::new` is exact.
            backdrop: RgbaF32::BLACK.with_alpha8(128),
            padding: Spacing::all(m.space.xl),
            min_width: 280.0,
        }
    }
}

impl Default for ModalTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
