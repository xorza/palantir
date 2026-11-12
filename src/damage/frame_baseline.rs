//! The frame-wide half of the damage baseline.

use crate::primitives::paint::color::RgbaF32;

/// What a presented frame was painted under besides its widgets: the clear
/// color, the font database and the loaded effects.
///
/// None of these belongs to a node, so no
/// [`NodeSnapshot`](crate::damage::node_snapshot::NodeSnapshot) diff sees
/// them move. The display is compared at frame entry in
/// `FrameRuntime::take_frame_plan`; these can change mid-frame, so they are
/// compared at frame end. Compared whole, so a new field is covered.
///
/// - Clear color: pixels outside partial-frame scissors keep their old
///   color, so a change needs a full repaint.
/// - A font load changes measurement and glyphs without moving any key
///   (`TextShaper::font_epoch`); unmoved rects would keep stale glyphs.
/// - An effect reload changes its pixels and keeps the id each shape hashes
///   (`EffectRegistry::epoch`); unmoved rects would keep the old effect.
/// - The collision outlines painted over everything: they move with the nodes they
///   mark and belong to none, so a frame that paints them repaints whole, and the first frame
///   after the last one clears them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameBaseline {
    pub(crate) clear: RgbaF32,
    pub(crate) font_epoch: u32,
    pub(crate) effect_epoch: u32,
    /// Whether the frame paints the explicit-id collision outlines.
    pub(crate) collision_overlay: bool,
}
