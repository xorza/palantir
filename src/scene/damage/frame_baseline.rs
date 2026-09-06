//! The frame-wide half of the damage baseline.

use crate::primitives::color::RgbaF32;

/// What a presented frame was painted under, apart from its widgets:
/// the colour behind them, and the font database that shaped them.
///
/// The per-widget half of the baseline is
/// [`DamageEngine::prev`](crate::scene::damage::DamageEngine::prev), and
/// every input it carries belongs to some node. These two belong to
/// none, so no
/// [`NodeSnapshot`](crate::scene::damage::node_snapshot::NodeSnapshot)
/// can hold them and no diff of snapshots can see either move. Both
/// decide what the surface looks like:
///
/// - The clear colour is what a full frame clears to, and what a partial
///   frame pre-fills each scissor with. Every pixel outside those
///   scissors keeps the colour it was already painted under, so a new
///   one reaches the screen through a full repaint or not at all.
/// - A font load changes what a run measures to, and what its glyphs
///   look like, without moving one key that addresses it — the reason
///   `TextShaper::font_epoch` exists. Rects that move are caught by the
///   diff like any other geometry change. Rects that do not move are
///   the ones that would keep their old glyphs.
///
/// The display is the peer that is deliberately *not* here. A host
/// supplies it at frame entry, where `FrameRuntime::take_frame_plan`
/// compares it against the last frame's stamp before anything runs.
/// These two can move *during* a frame — a theme swap in `App::update`,
/// a font load mid-record — so they are compared where the frame ends
/// instead, against what the last presented one used.
///
/// Compared whole, so an input added to this struct is an input the
/// comparison covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameBaseline {
    pub(crate) clear: RgbaF32,
    pub(crate) font_epoch: u32,
}
