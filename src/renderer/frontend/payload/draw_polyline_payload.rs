//! One stroked-polyline draw.

use crate::primitives::approx::noop_f32;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};

/// Stroked polyline payload. `width` is logical px. Points + colors
/// live in the window's [`RecordStore`] (`polyline_points` /
/// `polyline_colors`) — the payload only carries the spans.
/// `colors_len` is 1 (broadcast), `points_len` (per-point), or
/// `points_len - 1` (per-segment), selected by `color_mode`.
///
/// Points are stored **owner-local**; the composer applies `origin`
/// (the owner-rect top-left) before the active push-transform stack.
/// `bbox` is their owner-local centerline AABB; the composer applies
/// stroke/cap/join/AA inflation once in physical space.
///
/// [`RecordStore`]: crate::scene::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawPolylinePayload {
    /// Cull bound plus the spin, if any — set from a
    /// [`PaintAnim::Spin`] sample.
    ///
    /// [`PaintAnim::Spin`]: crate::scene::tree::paint_anims::PaintAnim::Spin
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: glam::Vec2,
    pub(crate) width: f32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
    pub(crate) color_mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
}

impl DrawPolylinePayload {
    /// Paints nothing when: fewer than two points (no segments) or a
    /// non-paintable stroke width.
    ///
    /// Unlike its siblings this is an **invariant**, not a filter —
    /// `PaintSink::draw_polyline` asserts it rather than gating on it,
    /// because both conditions are authoring-derived and already
    /// guaranteed by `Shape::Polyline::is_noop`. See that method for
    /// why the two differ.
    ///
    /// **Does not** check colour noop-ness: per-point / per-segment
    /// colours live in spans on the record store, and an O(n) read here
    /// would dominate the per-call cost. Colour noop is filtered at the
    /// `Shape::Polyline::is_noop` authoring boundary instead. The bbox
    /// can legitimately be zero-area (horizontal / vertical line) and
    /// still paint stroke pixels, so it isn't checked either.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.points_len < 2 || noop_f32(self.width)
    }
}
