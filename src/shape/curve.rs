//! The line, Bézier and arc builder: every geometry lowers to one `ShapeRecord::Curve`, with stroke properties travelling beside it.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::domain::vec2;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::lower;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::style::LineCap;
use glam::Vec2;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveGeometry {
    Line { a: Vec2, b: Vec2 },
    CubicBezier(CubicBezier),
    QuadraticBezier(QuadraticBezier),
    Arc(CircularArc),
}

impl CurveGeometry {
    /// The basis the curve pipeline traces. Lines and quadratics become cubics (exact promotion; a line's inner control points sit on the segment's thirds so `t` runs linearly and the composer keeps it one GPU instance). An arc keeps its own basis: the shader evaluates the exact circle.
    fn basis(self) -> CurveBasis {
        match self {
            Self::Line { a, b } => {
                let third = (b - a) / 3.0;
                CurveBasis::Cubic(CubicBezier {
                    p0: a,
                    p1: a + third,
                    p2: b - third,
                    p3: b,
                })
            }
            Self::CubicBezier(bezier) => CurveBasis::Cubic(bezier),
            Self::QuadraticBezier(bezier) => CurveBasis::Cubic(CubicBezier::from(bezier)),
            Self::Arc(arc) => {
                debug_assert!(
                    arc.sweep.abs() <= TAU,
                    "Shape::arc coerces its sweep to ±2π, not {}",
                    arc.sweep,
                );
                CurveBasis::Arc(arc)
            }
        }
    }
}

/// The stroke properties every geometry carries into lowering, so only the geometry varies between entry points.
#[derive(Clone, Copy, Debug)]
struct CurveStyle {
    stroke: Stroke,
    ramp: Option<ColorRamp>,
    cap: LineCap,
}

/// Stroked line, Bézier, or circular arc, centred on the curve like every path shape.
#[derive(Clone, Debug)]
#[must_use]
pub struct CurveShape {
    geometry: CurveGeometry,
    style: CurveStyle,
}

impl CurveShape {
    pub(super) const fn new(geometry: CurveGeometry, stroke: Stroke) -> Self {
        Self {
            geometry,
            style: CurveStyle {
                stroke,
                ramp: None,
                cap: LineCap::Butt,
            },
        }
    }

    /// Vary the color along the curve: at parameter `t` it is the stroke color times `ramp` at `t`, per channel (as a mesh tint). `t` runs 0 to 1 (`p0` → `p3` on a Bézier, across the sweep on an arc); it is not arc length, so color changes fastest where the curve moves least per step.
    pub const fn ramp(mut self, ramp: ColorRamp) -> Self {
        self.style.ramp = Some(ramp);
        self
    }

    /// How the two ends are finished.
    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.style.cap = cap;
        self
    }
}

impl sealed::LowerShape for CurveShape {
    fn is_noop(&self) -> bool {
        if self.style.stroke.is_noop() || self.style.ramp.is_some_and(|ramp| ramp.is_noop()) {
            return true;
        }
        match &self.geometry {
            CurveGeometry::Line { a, b } => vec2::approx_eq(*a, *b),
            CurveGeometry::CubicBezier(CubicBezier { p0, p1, p2, p3 }) => {
                vec2::approx_eq(*p0, *p1) && vec2::approx_eq(*p0, *p2) && vec2::approx_eq(*p0, *p3)
            }
            CurveGeometry::QuadraticBezier(QuadraticBezier { p0, p1, p2 }) => {
                vec2::approx_eq(*p0, *p1) && vec2::approx_eq(*p0, *p2)
            }
            CurveGeometry::Arc(CircularArc { radius, sweep, .. }) => {
                is_invisible(*radius) || is_invisible(sweep.abs())
            }
        }
    }

    /// The geometry is a few scalars, read directly rather than folded: the derived bbox would carry the NaN only after the ramp had interned. A ramp's integer-encoded stops hold no NaN.
    fn has_nan(&self) -> bool {
        let geometry = match &self.geometry {
            CurveGeometry::Line { a, b } => a.has_nan() || b.has_nan(),
            CurveGeometry::CubicBezier(CubicBezier { p0, p1, p2, p3 }) => {
                p0.has_nan() || p1.has_nan() || p2.has_nan() || p3.has_nan()
            }
            CurveGeometry::QuadraticBezier(QuadraticBezier { p0, p1, p2 }) => {
                p0.has_nan() || p1.has_nan() || p2.has_nan()
            }
            CurveGeometry::Arc(CircularArc {
                center,
                radius,
                start_angle,
                sweep,
            }) => center.has_nan() || radius.is_nan() || start_angle.is_nan() || sweep.is_nan(),
        };
        geometry || self.style.stroke.has_nan()
    }

    /// The geometry's [`CurveBasis`] plus its tight bbox; tessellation is GPU-side at draw time.
    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            geometry,
            style: CurveStyle { stroke, ramp, cap },
        } = self;
        let basis = geometry.basis();
        ShapeRecord::Curve {
            cap,
            basis,
            stroke: ShapeStroke::from(stroke),
            bbox: basis.bbox(),
            ramp: ramp.map_or(CurveRamp::None, |ramp| lower::ramp(store, &ramp)),
        }
    }
}
