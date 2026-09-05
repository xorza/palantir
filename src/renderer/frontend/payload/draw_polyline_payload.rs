//! One stroked-polyline draw.

use crate::primitives::approx::noop_f32;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;

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
    /// Cull bound plus the turn, if any — set from a paint animation's
    /// sampled rotation.
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: Vec2,
    pub(crate) width: f32,
    pub(crate) points_start: u32,
    pub(crate) points_len: u32,
    pub(crate) colors_start: u32,
    pub(crate) colors_len: u32,
    pub(crate) color_mode: ColorMode,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    /// Opacity multiplier from a paint animation, `255` for a still
    /// polyline.
    ///
    /// A lane rather than a scaled colour, because the colours are a span
    /// in the record store this payload only points at — scaling them
    /// here would mean copying the run every frame. The composer folds it
    /// in as it writes each curve instance, where it is already touching
    /// every colour once.
    ///
    /// Eight bits, not a float: the colours it multiplies are `RgbaU8`,
    /// so the extra precision has nowhere to land, and the byte rides in
    /// this payload's tail padding instead of growing it.
    pub(crate) alpha: u8,
}

impl DrawPolylinePayload {
    /// This draw with its alpha scaled by `by`, for
    /// [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)'s
    /// gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            alpha: (f32::from(self.alpha) * by).round().clamp(0.0, 255.0) as u8,
            ..self
        }
    }

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
