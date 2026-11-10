//! One stroke draw — a cubic, an arc or a polyline, all on the one curve pipeline.

use crate::common::span::Span;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use glam::Vec2;

/// A stroke along `path`, owner-local; `width` is logical px. The composer turns the path by
/// `spin`, culls it by its turned bounds grown by the stroke, cap, joins and AA, adds `origin` and
/// the push-transform stack, scales to physical px and pushes `CurveInstance`s onto
/// `RenderBuffer.curves`. Refused: a payload for each kind of path, as the three draw on one
/// pipeline with one paint.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawStrokePayload {
    pub(crate) path: StrokePath,
    pub(crate) origin: Vec2,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    /// Solid or ramp ([`GpuFill::curve`]); a polyline's is solid, and its colors multiply it.
    pub(crate) fill: GpuFill,
    pub(crate) width: f32,
    pub(crate) cap: LineCap,
}

/// What a stroke traces, owner-local.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StrokePath {
    /// A cubic or an arc, traced whole. The composer turns its basis about the pivot exactly: a
    /// Bézier by affine invariance, a circle by moving its center and shifting both angles.
    Curve(CurveBasis),
    Polyline(PolylinePath),
}

/// A polyline's points and colors in the window's [`RecordStore`], as recorded; the composer fits
/// them by `fit` into the owner-local frame, turns them, then applies the payload's `origin`.
///
/// [`RecordStore`]: crate::shape::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PolylinePath {
    /// The fitted points' owner-local bounds, kept as a scan of the points is O(n).
    pub(crate) bbox: Rect,
    pub(crate) fit: FitMap,
    pub(crate) points: Span,
    pub(crate) colors: PolylineColorSpan,
    pub(crate) join: LineJoin,
}

impl Default for StrokePath {
    /// A degenerate cubic at the origin: the `Default` for a `DrawStrokePayload` literal, never
    /// drawn.
    fn default() -> Self {
        Self::Curve(CurveBasis::default())
    }
}

impl PaintPayload for DrawStrokePayload {
    /// This draw with alpha scaled by `by`, for [`PaintSink`]'s gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            ..self
        }
    }

    /// Paints nothing for a non-positive stroke width, a degenerate arc radius, a polyline of
    /// fewer than two points, or a transparent stroke color. An all-transparent ramp is caught
    /// earlier by `CurveShape`'s no-op test, and all-transparent polyline colors by the polyline's
    /// lowering; the polyline's colors are not read here, an O(n) span read.
    #[inline]
    fn is_noop(&self) -> bool {
        let degenerate = match self.path {
            StrokePath::Curve(CurveBasis::Arc(arc)) => is_invisible(arc.radius),
            StrokePath::Curve(CurveBasis::Cubic(_)) => false,
            StrokePath::Polyline(polyline) => polyline.points.len < 2,
        };
        degenerate || is_invisible(self.width) || self.fill.is_noop()
    }

    fn spin_mut(&mut self) -> &mut Option<Spin> {
        &mut self.spin
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.stroke(self);
    }

    /// A polyline's geometry is asserted, not gated: its lowering drops fewer than two points and
    /// a non-painting width, and the encoder forwards both verbatim. Debug-only is safe: the
    /// composer emits nothing for a polyline of fewer than two kept points.
    fn check(&self) {
        if let StrokePath::Polyline(polyline) = self.path {
            debug_assert!(
                polyline.points.len >= 2 && !is_invisible(self.width),
                "degenerate polyline reached the sink — its lowering should have dropped it: \
                 {self:?}",
            );
        }
    }
}
