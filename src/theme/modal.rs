//! What a modal wears: the dialog surface and the scrim that dims what is behind it.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::widgets::modal::Modal`]. Builder overrides (`.chrome(...)` / `.scrim(...)`) win; these defaults fill in.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
pub struct ModalTheme {
    /// Dialog panel chrome.
    pub panel: Chrome,
    /// Dimming scrim; straight-alpha linear. A color, as Fluent's ContentDialog smoke layer,
    /// Material 3's scrim, Apple's alerts and libadwaita's `AdwDialog` all dim; a frosted panel
    /// shows the page through itself. Refused: a blurred scrim, which captures the whole window.
    pub scrim: RgbaF32,
    /// Padding inside the panel, applied when the builder leaves it unset.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub padding: Spacing,
    /// Minimum panel width in logical px; the panel hugs its content above it.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub min_width: f32,
}

impl ModalTheme {
    /// A floating panel over a half-opaque black scrim, in [`Metrics::float_material`] held for the
    /// ambient ink, as the modal's content is the app's.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let panel = Chrome::rounded(p.element_mid, Corners::all(m.radius.float))
            .with_border(Stroke::new(p.border_mid(), m.stroke.hairline))
            .with_shadow(p.float_shadow(m));
        Self {
            panel: m.dress_float(panel, p.text),
            scrim: RgbaF32::BLACK.with_alpha8(128),
            padding: Spacing::all(m.space.xl),
            min_width: 280.0,
        }
    }
}
