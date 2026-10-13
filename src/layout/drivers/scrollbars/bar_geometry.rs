//! One scrollbar along its axis.

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
