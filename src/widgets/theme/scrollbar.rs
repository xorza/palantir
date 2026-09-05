//! What a scroll's bars wear, in both the modes they can lay out in —
//! reserved beside the content, or floating over it.

use crate::primitives::color::RgbaF32;
use crate::widgets::theme::palette::Palette;

/// Visuals for [`crate::Scroll`] reservation-layout scrollbars. Under
/// [`BarMode::Reserved`](crate::BarMode) the widget takes `thickness`
/// of padding off each panned axis's far edge whether or not anything
/// currently overflows, and the bar paints in that reserved strip —
/// beside the visible content, never on top of it. Track + thumb are
/// pill-capped filled rects, and the thumb fill picks between `thumb` /
/// `thumb_hovered` / `thumb_active` on the bar leaf's hover + drag
/// state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScrollbarTheme {
    /// Cross-axis thickness of the bar in logical px. The pill radius
    /// of track and thumb is `thickness / 2`.
    pub thickness: f32,
    /// Empty padding strip between content and the bar. Reserved
    /// alongside `thickness` (total reservation = `thickness + gap`) but
    /// painted as nothing — pure breathing room so the bar doesn't
    /// touch the visible content.
    pub gap: f32,
    /// Floor for the thumb's main-axis length so a tiny `viewport /
    /// content` ratio doesn't produce an ungrabbable nub.
    pub min_thumb_px: f32,
    /// Track background. `RgbaF32::TRANSPARENT` = pure overlay (only the
    /// thumb is visible) — the macOS-style default.
    pub track: RgbaF32,
    /// Idle thumb fill.
    pub thumb: RgbaF32,
    /// Thumb fill while the pointer is over the bar.
    pub thumb_hovered: RgbaF32,
    /// Thumb fill while the thumb is drag-captured (or pressed).
    pub thumb_active: RgbaF32,
}

impl ScrollbarTheme {
    /// The palette defines no scrollbar colors; use `text_muted` at
    /// decreasing translucency for idle / hover / active so the bar
    /// reads as a soft overlay matching the palette's muted-text gray
    /// rather than pure black.
    pub fn from_palette(p: &Palette) -> Self {
        let thumb = |alpha: f32| p.text_muted.with_alpha(alpha);
        Self {
            thickness: 8.0,
            gap: 4.0,
            min_thumb_px: 24.0,
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
