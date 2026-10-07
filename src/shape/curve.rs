//! The line, Bézier and arc builder: every geometry lowers to one `ShapeRecord::Curve`, with stroke properties travelling beside it.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::domain::vec2;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::lower;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::style::LineCap;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveGeometry {
    Line {
        a: Vec2,
        b: Vec2,
    },
    CubicBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    QuadraticBezier {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
    },
    Arc {
        center: Vec2,
        radius: f32,
        start_angle: f32,
        sweep: f32,
    },
}

/// The stroke properties every geometry carries into lowering, so only the geometry varies between entry points.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CurveStyle {
    pub(crate) stroke: Stroke,
    pub(crate) ramp: Option<ColorRamp>,
    pub(crate) cap: LineCap,
}

/// Stroked line, Bézier, or circular arc, centred on the curve like every path shape.
#[derive(Clone, Debug)]
#[must_use]
pub struct CurveShape {
    pub(crate) geometry: CurveGeometry,
    pub(crate) style: CurveStyle,
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
}

impl CurveShape {
    /// Vary the colour along the curve: at parameter `t` it is the stroke colour times `ramp` at `t`, per channel (as a mesh tint). `t` runs 0 to 1 (`p0` → `p3` on a Bézier, across the sweep on an arc); it is not arc length, so colour changes fastest where the curve moves least per step.
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
            CurveGeometry::CubicBezier { p0, p1, p2, p3 } => {
                vec2::approx_eq(*p0, *p1) && vec2::approx_eq(*p0, *p2) && vec2::approx_eq(*p0, *p3)
            }
            CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
                vec2::approx_eq(*p0, *p1) && vec2::approx_eq(*p0, *p2)
            }
            CurveGeometry::Arc { radius, sweep, .. } => {
                is_invisible(*radius) || is_invisible(sweep.abs())
            }
        }
    }

    /// The geometry is a few scalars, read directly rather than folded: the derived bbox would carry the NaN only after the ramp had interned. A ramp's integer-encoded stops hold no NaN.
    fn has_nan(&self) -> bool {
        let geometry = match &self.geometry {
            CurveGeometry::Line { a, b } => a.has_nan() || b.has_nan(),
            CurveGeometry::CubicBezier { p0, p1, p2, p3 } => {
                p0.has_nan() || p1.has_nan() || p2.has_nan() || p3.has_nan()
            }
            CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
                p0.has_nan() || p1.has_nan() || p2.has_nan()
            }
            CurveGeometry::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => center.has_nan() || radius.is_nan() || start_angle.is_nan() || sweep.is_nan(),
        };
        geometry || self.style.stroke.has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        lower::curve(store, self.geometry, self.style)
    }
}
