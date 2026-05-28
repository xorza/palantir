use crate::primitives::color::Color;
use crate::widgets::theme::palette;

/// Visuals for [`crate::Scroll`] reservation-layout scrollbars. When
/// content overflows on a panned axis, the widget reserves `width`
/// of padding on that axis's far edge; the bar paints in the reserved
/// strip — beside the visible content, never on top of it. Track +
/// thumb are filled rounded rects. The thumb fill picks between
/// `thumb` / `thumb_hover` / `thumb_active` based on the bar leaf's
/// hover + drag state (see `scroll::push_bar_nodes`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScrollbarTheme {
    /// Cross-axis thickness of the bar in logical px.
    pub width: f32,
    /// Empty padding strip between content and the bar. Reserved
    /// alongside `width` (total reservation = `width + gap`) but
    /// painted as nothing — pure breathing room so the bar doesn't
    /// touch the visible content.
    pub gap: f32,
    /// Floor for the thumb's main-axis length so a tiny `viewport /
    /// content` ratio doesn't produce an ungrabbable nub.
    pub min_thumb_px: f32,
    /// Track background. `Color::TRANSPARENT` = pure overlay (only the
    /// thumb is visible) — the macOS-style default.
    pub track: Color,
    /// Idle thumb fill.
    pub thumb: Color,
    /// Thumb fill while the pointer is over the bar.
    pub thumb_hover: Color,
    /// Thumb fill while the thumb is drag-captured (or pressed).
    pub thumb_active: Color,
    /// Corner radius applied to track and thumb. `width / 2` = pill.
    pub radius: f32,
}

impl Default for ScrollbarTheme {
    fn default() -> Self {
        // Ayu doesn't define scrollbar colors directly. Use TEXT_MUTED
        // at decreasing translucency for idle / hover / active so the
        // bar reads as a soft overlay matching the palette's
        // muted-text gray rather than pure black.
        let thumb = |alpha: f32| {
            let m = palette::TEXT_MUTED;
            m.with_alpha(alpha)
        };
        Self {
            width: 8.0,
            gap: 4.0,
            min_thumb_px: 24.0,
            track: Color::TRANSPARENT,
            thumb: thumb(0.45),
            thumb_hover: thumb(0.65),
            thumb_active: thumb(0.85),
            radius: 4.0,
        }
    }
}
