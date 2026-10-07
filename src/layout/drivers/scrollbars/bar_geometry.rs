//! One scrollbar along its axis.

use crate::primitives::math::domain;
use crate::primitives::math::num::F32Px;

/// One bar along its axis, in logical pixels from the track's start. Thumb size and offset are whole pixels, so paint and pointer mapping cannot disagree by a rounding.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct BarGeometry {
    /// The track's length: the viewport's extent on this axis, and one page of a track click.
    pub(crate) track: f32,
    /// The thumb's length.
    pub(crate) thumb_size: f32,
    /// Where the thumb starts along the track.
    pub(crate) thumb_offset: f32,
    /// How far the thumb can slide: the track floored to whole pixels, less the thumb. Carried because the reader has only the unfloored [`Self::track`], which put a drag up to a pixel off the thumb's real travel.
    pub(crate) travel: f32,
    /// The largest offset the bar shows, where the content's far edge meets the track's; the thumb covers `0.0..=max_offset`. A wheel can go past into a [`Scroll::content_margin`](crate::Scroll::content_margin) band the thumb does not show.
    pub(crate) max_offset: f32,
}

impl BarGeometry {
    /// `track` is both the ratio the thumb expresses and, floored to
    /// whole logical pixels, the length it slides along.
    pub(super) fn along(track: f32, content: f32, offset: f32, min_thumb: f32) -> Option<Self> {
        if track <= 0.0 || content <= track {
            return None;
        }
        // Quantized here, not at paint: the widget reads these back to map a drag or track click, so the grabbed bar must be the drawn bar. Whole logical pixels because physical snapping rounds a rect's min and max independently (`Rect::scaled_by`), so a thumb on fractional coordinates would change length as it travels; integer edges pin the length at integer DPR.
        // **One length, not two**: `travel` comes off the same floored length the thumb clamped into; flooring separately let a sub-pixel track take the 1 px minimum thumb and place it at -1.
        let floored = track.floor().max(1.0);
        let thumb_size = (track / content * track)
            .max(min_thumb)
            .fast_round()
            .clamp(1.0, floored);
        let travel = floored - thumb_size;
        let max_offset = content - track;
        let fraction = domain::share_of(offset, max_offset).clamp(0.0, 1.0);
        Some(Self {
            track,
            thumb_size,
            thumb_offset: (fraction * travel).fast_round(),
            travel,
            max_offset,
        })
    }
}
