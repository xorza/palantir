//! What a modal wears: the dialog surface and the scrim that dims what is behind it.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::widgets::modal::Modal`]. Builder overrides (`.chrome(...)` / `.scrim(...)`) win; these defaults fill in.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
pub struct ModalTheme {
    /// Dialog panel chrome.
    pub panel: Chrome,
    /// The box the modal lays over the window behind its panel: a fill that dims it, as Fluent's
    /// ContentDialog smoke layer, Material 3's scrim, Apple's alerts and libadwaita's `AdwDialog`
    /// do, and a material that blurs it too, as CSS's `::backdrop` takes a `backdrop-filter`. A
    /// material costs a capture and a blur of the whole window on each frame where something under
    /// the scrim changes: 394 µs and 732 µs of device time at 3840 × 2160 on a mobile AMD iGPU
    /// (`integration/backdrop_frame/gpu/modal`, `.notes/research/2026-10-28-backdrops.md`), and
    /// nothing on a frame where nothing does. A scrim's material names the scrim's own dim as its
    /// fallback, so it keeps dimming while transparency is reduced, as iOS keeps it, where the
    /// theme's surface would hide the window.
    pub scrim: Chrome,
    /// Padding inside the panel, applied when the builder leaves it unset.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub padding: Spacing,
    /// Minimum panel width in logical px; the panel hugs its content above it.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub min_width: f32,
}

impl ModalTheme {
    /// [`Palette::popup_panel`] over a half-opaque black scrim that only dims, as every native
    /// toolkit's does, in [`Metrics::float_material`]
    /// held for the ambient ink, as the modal's content is the app's. The panel sits a rung under
    /// the controls it holds, as every surface does, so a button shows by its fill. Refused:
    /// `element_mid`, a button's own fill, which left a button its hairline alone.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            panel: m.dress_float(p.popup_panel(m), p.text),
            scrim: Chrome::new(RgbaF32::BLACK.with_alpha8(128)),
            padding: Spacing::all(m.space.xl),
            min_width: 280.0,
        }
    }
}
