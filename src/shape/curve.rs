//! The line, Bézier and arc builder. Every geometry lowers to one
//! `ShapeRecord::Curve`, and the stroke properties travel beside the
//! geometry so only the geometry varies between the entry points.

use crate::primitives::approx::{paints_nothing, vec2_approx_eq};
use crate::primitives::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::nan::NanCheck;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::style::LineCap;
use glam::Vec2;

#[derive(Clone, Debug)]
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

/// The stroke properties every curve geometry carries into lowering. They travel
/// together from the setters to the lowering entry points, so the geometry is the
/// only thing that varies between them.
#[derive(Clone, Debug)]
pub(crate) struct CurveStyle {
    pub(crate) stroke: Stroke,
    pub(crate) ramp: Option<ColorRamp>,
    pub(crate) cap: LineCap,
}

/// Stroked line, Bézier, or circular arc. The stroke is centred on the
/// curve, like every path shape's.
#[derive(Clone, Debug)]
pub struct CurveShape {
    pub(crate) geometry: CurveGeometry,
    pub(crate) style: CurveStyle,
}

impl CurveShape {
    pub(super) fn new(geometry: CurveGeometry, stroke: Stroke) -> Self {
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
    /// Vary the colour along the curve: at curve parameter `t` it is the
    /// stroke colour times `ramp` at `t`, channel by channel — the rule a
    /// mesh tint follows. `t` runs from 0 at the start to 1 at the end:
    /// `p0` → `p3` on a Bézier, across the sweep on an arc. It is the
    /// curve parameter, not arc length: on a Bézier whose control points
    /// are unevenly spaced, the colour changes fastest where the curve
    /// moves least per step of `t`.
    pub fn ramp(mut self, ramp: impl Into<ColorRamp>) -> Self {
        self.style.ramp = Some(ramp.into());
        self
    }

    /// How the two ends are finished.
    pub fn cap(mut self, cap: impl Into<LineCap>) -> Self {
        self.style.cap = cap.into();
        self
    }
}

impl sealed::LowerShape for CurveShape {
    fn is_noop(&self) -> bool {
        if self.style.stroke.is_noop() || self.style.ramp.is_some_and(|ramp| ramp.is_noop()) {
            return true;
        }
        match &self.geometry {
            CurveGeometry::Line { a, b } => vec2_approx_eq(*a, *b),
            CurveGeometry::CubicBezier { p0, p1, p2, p3 } => {
                vec2_approx_eq(*p0, *p1) && vec2_approx_eq(*p0, *p2) && vec2_approx_eq(*p0, *p3)
            }
            CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
                vec2_approx_eq(*p0, *p1) && vec2_approx_eq(*p0, *p2)
            }
            CurveGeometry::Arc { radius, sweep, .. } => {
                paints_nothing(*radius) || paints_nothing(sweep.abs())
            }
        }
    }

    /// The geometry is a fixed handful of scalars, so they are read
    /// directly rather than through a fold — the bbox lowering derives
    /// from them would carry the NaN too, but only after the ramp had
    /// interned into the store. A ramp's stops are integer-encoded, so
    /// it holds no NaN of its own.
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
