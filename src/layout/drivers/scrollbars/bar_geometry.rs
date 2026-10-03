//! One scrollbar along its axis.

use crate::primitives::math::approx;
use crate::primitives::math::num::F32Px;

/// One bar along its axis, in logical pixels from the track's start:
/// what [`ScrollbarsDef::thumb`](crate::layout::drivers::scrollbars::scrollbars_def::ScrollbarsDef::thumb) answers.
///
/// The thumb's size and offset are whole pixels already. The driver paints
/// them as they are, and a widget maps pointer input against the same
/// numbers, so the two cannot disagree by a rounding.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct BarGeometry {
    /// The track's length: the viewport's extent on this axis, and the
    /// distance one page of a track click moves the content.
    pub(crate) track: f32,
    /// The thumb's length.
    pub(crate) thumb_size: f32,
    /// Where the thumb starts along the track.
    pub(crate) thumb_offset: f32,
    /// How far the thumb can slide: the track floored to whole pixels,
    /// less the thumb.
    ///
    /// Carried rather than left to the reader, because the reader has
    /// only the unfloored [`Self::track`] to subtract from. A fractional
    /// viewport put that denominator up to a pixel away from the distance
    /// the thumb moves, so a drag scrubbed the content at slightly the
    /// wrong rate.
    pub(crate) travel: f32,
    /// The largest offset the bar shows, where the content's far edge
    /// meets the track's. The thumb covers `0.0..=max_offset`. A wheel can
    /// go past either end into a
    /// [`Scroll::content_margin`](crate::Scroll::content_margin) band,
    /// which the thumb does not show.
    pub(crate) max_offset: f32,
}

impl BarGeometry {
    /// `track` is both the ratio the thumb expresses and, floored to
    /// whole logical pixels, the length it slides along.
    pub(super) fn along(track: f32, content: f32, offset: f32, min_thumb: f32) -> Option<Self> {
        if track <= 0.0 || content <= track {
            return None;
        }
        // Quantized here rather than at the paint site: the widget reads
        // these back to map a drag or a track click onto an offset, so the
        // bar the user grabs has to be the bar that was drawn. Rounding on
        // one side only put drag scaling up to a pixel out and made a click
        // in the rounding sliver page away from the thumb.
        //
        // Whole logical pixels because physical snapping rounds a rect's
        // min and max *independently* (`Rect::scaled_by`), which keeps
        // adjacent rects flush but makes a rect's snapped *length* depend on
        // where it sits — so a thumb on fractional coordinates visibly grows
        // and shrinks by a pixel as it travels. Integer logical edges scale
        // to integer physical ones at integer DPR, which pins the length; at
        // fractional DPR it only narrows the wobble, since the real cause is
        // in the snap.
        //
        // **One length, not two.** `travel` comes off the same floored
        // length the thumb clamped into, so a 0..1 fraction of it lands in
        // `0..=travel` and needs no clamp of its own. Flooring the track
        // separately for each cap let a sub-pixel track floor to zero, take
        // the one-pixel minimum thumb, and place it at -1.
        let floored = track.floor().max(1.0);
        let thumb_size = (track / content * track)
            .max(min_thumb)
            .fast_round()
            .clamp(1.0, floored);
        let travel = floored - thumb_size;
        let max_offset = content - track;
        let fraction = approx::share_of(offset, max_offset).clamp(0.0, 1.0);
        Some(Self {
            track,
            thumb_size,
            thumb_offset: (fraction * travel).fast_round(),
            travel,
            max_offset,
        })
    }
}
