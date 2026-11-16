//! One stroke draw — a cubic, an arc or a polyline, all on the one curve pipeline.

use crate::common::span::Span;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::line_style::{Dash, LineCap, LineJoin};
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::gpu_fill::GpuFill;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use glam::Vec2;

/// A stroke along `path`, owner-local; `width` is logical px. The composer turns the path by
/// the draw's turn, culls it by its turned bounds grown by the stroke, cap, joins and AA, adds `origin` and
/// the push-transform stack, scales to physical px and pushes `CurveInstance`s onto
/// `RenderBuffer.curves`. Refused: a payload for each kind of path, as the three draw on one
/// pipeline with one paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawStrokePayload {
    pub(crate) path: StrokePath,
    pub(crate) origin: Vec2,
    /// Solid or ramp ([`GpuFill::curve`]); a polyline's is solid, and its colors multiply it.
    pub(crate) fill: GpuFill,
    pub(crate) width: f32,
    pub(crate) cap: LineCap,
    /// The dash pattern in logical px, which a curve breaks into; never a solid one, which the
    /// stroke's lowering drops, and never on a polyline, which draws none.
    pub(crate) dash: Option<Dash>,
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

impl PaintPayload for DrawStrokePayload {
    /// This draw with alpha scaled by `by`, for [`PaintSink`]'s gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
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

    fn paint(self, spin: Option<Spin>, sink: &mut impl PaintSink) {
        sink.stroke(self, spin);
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::primitives::geometry::cubic_bezier::CubicBezier;
    use crate::primitives::paint::line_style::LineCap;
    use crate::renderer::frontend::payload::draw_stroke_payload::{DrawStrokePayload, StrokePath};
    use crate::renderer::render_buffer::gpu_fill::GpuFill;
    use crate::shape::geometry::curve_basis::CurveBasis;
    use glam::Vec2;

    impl DrawStrokePayload {
        /// A degenerate cubic at the origin, of no width and no color, still: the base of a test's
        /// or a bench's literal, which sets what it draws.
        pub(crate) fn fixture() -> Self {
            Self {
                path: StrokePath::Curve(CurveBasis::Cubic(CubicBezier {
                    p0: Vec2::ZERO,
                    p1: Vec2::ZERO,
                    p2: Vec2::ZERO,
                    p3: Vec2::ZERO,
                })),
                origin: Vec2::ZERO,
                fill: GpuFill::default(),
                width: 0.0,
                cap: LineCap::default(),
                dash: None,
            }
        }
    }
}
