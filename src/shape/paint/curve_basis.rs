//! The geometry of a lowered curve.

use glam::Vec2;

/// Which parametric basis a stroke traces: the half that differs between a Bézier and an arc. Both lower to a `CurveInstance` on the one curve pipeline, selected by its `kind` lane.
///
/// Lowered once in [`crate::shape::lower`] and carried verbatim from [`ShapeRecord::Curve`] through `DrawCurvePayload` to the composer. Owner-local: the composer folds in origin and transform before scaling to physical px.
///
/// [`ShapeRecord::Curve`]: crate::shape::record::ShapeRecord::Curve
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveBasis {
    /// Cubic Bézier control points. Quadratics promote and lines degenerate to this at lowering.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Exact circle; ramp `t` tracks the sweep linearly. `a0`/`a1` are radians in screen convention (0 = +x, y-down, so increasing is clockwise); `a1 < a0` is a negative sweep.
    Arc {
        center: Vec2,
        radius: f32,
        a0: f32,
        a1: f32,
    },
}

impl Default for CurveBasis {
    /// A degenerate cubic at the origin: the `Default` for a `DrawCurvePayload` literal, never drawn.
    fn default() -> Self {
        Self::Cubic {
            p0: Vec2::ZERO,
            p1: Vec2::ZERO,
            p2: Vec2::ZERO,
            p3: Vec2::ZERO,
        }
    }
}
