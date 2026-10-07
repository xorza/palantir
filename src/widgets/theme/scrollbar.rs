//! What a scroll's bars wear, reserved beside the content or floating over it.

use crate::primitives::paint::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Scroll`] reservation-layout scrollbars. Under [`BarMode::Reserved`](crate::BarMode) the widget takes `thickness` of padding off each panned axis's far edge and paints the bar there. Track and thumb are pill-capped; the thumb fill follows hover and drag state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScrollbarTheme {
    /// Cross-axis thickness in logical px; pill radius is `thickness / 2`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub thickness: f32,
    /// Empty strip between content and bar; reserved in addition to `thickness`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub gap: f32,
    /// Floor for the thumb's main-axis length.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_thumb: f32,
    /// Track background; `TRANSPARENT` is pure overlay (macOS-style default).
    pub track: RgbaF32,
    /// Idle thumb fill.
    pub thumb: RgbaF32,
    /// Thumb fill while the pointer is over the bar.
    pub thumb_hovered: RgbaF32,
    /// Thumb fill while drag-captured or pressed.
    pub thumb_active: RgbaF32,
}

impl ScrollbarTheme {
    /// The palette defines no scrollbar colors; derived from `text_muted` at decreasing translucency.
    pub fn from_palette(p: &Palette) -> Self {
        let thumb = |alpha: f32| p.text_muted.with_alpha(alpha);
        Self {
            thickness: 8.0,
            gap: 4.0,
            min_thumb: 24.0,
            track: RgbaF32::TRANSPARENT,
            thumb: thumb(0.45),
            thumb_hovered: thumb(0.65),
            thumb_active: thumb(0.85),
        }
    }
}

impl Default for ScrollbarTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
