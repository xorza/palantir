//! The line, Bézier and arc builder: every geometry lowers to one `ShapeRecord::Curve`, with stroke properties traveling beside it.

use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::rect::RectKind;
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
    /// The one point a curve of no length covers, or `None` for a curve with length. An arc of no
    /// radius sits at its center, and one of no sweep at its start.
    fn point(self) -> Option<Vec2> {
        match self {
            Self::Line { a, b } => vec2::approx_eq(a, b).then_some(a),
            Self::CubicBezier(CubicBezier { p0, p1, p2, p3 }) => {
                (vec2::approx_eq(p0, p1) && vec2::approx_eq(p0, p2) && vec2::approx_eq(p0, p3))
                    .then_some(p0)
            }
            Self::QuadraticBezier(QuadraticBezier { p0, p1, p2 }) => {
                (vec2::approx_eq(p0, p1) && vec2::approx_eq(p0, p2)).then_some(p0)
            }
            Self::Arc(CircularArc {
                center,
                radius,
                start_angle,
                sweep,
            }) => {
                if is_invisible(radius) {
                    Some(center)
                } else if is_invisible(sweep.abs()) {
                    Some(center + radius * Vec2::from_angle(start_angle))
                } else {
                    None
                }
            }
        }
    }

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

/// Stroked line, Bézier, or circular arc, centered on the curve like every path shape.
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
        // A curve of no length paints its caps alone, and a butt cap has no extent.
        self.style.cap == LineCap::Butt && self.geometry.point().is_some()
    }

    /// The geometry's [`CurveBasis`] plus its tight bbox; tessellation is GPU-side at draw time.
    /// A curve of no length lowers to its cap alone, as SVG paints a zero-length subpath: a disc
    /// for a round cap, an axis-aligned square for a square one, the stroke width across. Being a
    /// quad, the dot does not follow a `turn` animation.
    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            geometry,
            style: CurveStyle { stroke, ramp, cap },
        } = self;
        if let Some(point) = geometry.point() {
            let color = ramp.map_or(stroke.color, |ramp| stroke.color.tinted(ramp.color_at(0.0)));
            let half = stroke.width * 0.5;
            let corners = match cap {
                LineCap::Round => Corners::all(half),
                LineCap::Square => Corners::ZERO,
                LineCap::Butt => unreachable!("a zero-length butt-capped curve is a no-op"),
            };
            return ShapeRecord::Quad(QuadShape::Rect {
                kind: RectKind::Rounded,
                local_rect: Some(Rect::new(
                    point.x - half,
                    point.y - half,
                    stroke.width,
                    stroke.width,
                )),
                corners,
                fill: lower::brush(store, &Brush::Solid(color)),
                border: ShapeStroke::from(Stroke::NONE),
            });
        }
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
