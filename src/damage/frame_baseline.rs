//! The frame-wide half of the damage baseline.

use crate::primitives::paint::color::RgbaF32;

/// What a presented frame was painted under besides its widgets: the clear
/// colour and the font database.
///
/// Neither belongs to a node, so no
/// [`NodeSnapshot`](crate::damage::node_snapshot::NodeSnapshot) diff sees
/// them move. The display is compared at frame entry in
/// `FrameRuntime::take_frame_plan`; these can change mid-frame, so they are
/// compared at frame end. Compared whole, so a new field is covered.
///
/// - Clear colour: pixels outside partial-frame scissors keep their old
///   colour, so a change needs a full repaint.
/// - A font load changes measurement and glyphs without moving any key
///   (`TextShaper::font_epoch`); unmoved rects would keep stale glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameBaseline {
    pub(crate) clear: RgbaF32,
    pub(crate) font_epoch: u32,
}
