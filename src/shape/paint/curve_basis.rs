//! The geometry of a lowered curve.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

/// Which parametric basis a stroke traces: the half that differs between a Bézier and an arc. Both lower to a `CurveInstance` on the one curve pipeline, selected by its `kind` lane.
///
/// Lowered once by [`CurveShape`](crate::widget::CurveShape) and carried verbatim from [`ShapeRecord::Curve`] through `DrawCurvePayload` to the composer. Owner-local: the composer folds in origin and transform before scaling to physical px.
///
/// [`ShapeRecord::Curve`]: crate::shape::record::ShapeRecord::Curve
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveBasis {
    /// Cubic Bézier control points. Quadratics promote and lines degenerate to this at lowering.
    Cubic(CubicBezier),
    /// Exact circle; ramp `t` tracks the sweep linearly.
    Arc(CircularArc),
}

impl CurveBasis {
    /// Tight bbox of the centerline, no stroke inflation.
    pub(crate) fn bbox(self) -> Rect {
        match self {
            Self::Cubic(bezier) => bezier.bbox(),
            Self::Arc(arc) => arc.bbox(),
        }
    }
}

impl Default for CurveBasis {
    /// A degenerate cubic at the origin: the `Default` for a `DrawCurvePayload` literal, never drawn.
    fn default() -> Self {
        Self::Cubic(CubicBezier {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        })
    }
}
