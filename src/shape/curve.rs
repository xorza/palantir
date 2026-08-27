//! The line, Bézier and arc builder. Every geometry lowers to one
//! `ShapeRecord::Curve`, and the stroke properties travel beside the
//! geometry so only the geometry varies between the entry points.

use crate::primitives::approx::{noop_f32, vec2_approx_eq};
use crate::primitives::brush::CurveBrush;
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
pub(crate) struct CurveStroke {
    pub(crate) width: f32,
    pub(crate) brush: CurveBrush,
    pub(crate) cap: LineCap,
}

/// Stroked line, Bézier, or circular arc.
#[derive(Clone, Debug)]
pub struct CurveShape {
    pub(crate) geometry: CurveGeometry,
    pub(crate) stroke: CurveStroke,
}

impl CurveShape {
    pub(super) fn new(geometry: CurveGeometry, width: f32) -> Self {
        Self {
            geometry,
            stroke: CurveStroke {
                width,
                brush: CurveBrush::TRANSPARENT,
                cap: LineCap::Butt,
            },
        }
    }
}

shape_setters!(CurveShape {
    brush: CurveBrush => stroke.brush,
    cap: LineCap => stroke.cap,
});

impl sealed::LowerShape for CurveShape {
    fn is_noop(&self) -> bool {
        if noop_f32(self.stroke.width) || self.stroke.brush.as_brush().is_noop() {
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
            CurveGeometry::Arc { radius, sweep, .. } => noop_f32(*radius) || noop_f32(sweep.abs()),
        }
    }

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        let Self { geometry, stroke } = self;
        match geometry {
            CurveGeometry::Line { a, b } => lower::line(store, a, b, stroke),
            CurveGeometry::CubicBezier { p0, p1, p2, p3 } => {
                lower::cubic_bezier(store, [p0, p1, p2, p3], stroke)
            }
            CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
                lower::quadratic_bezier(store, [p0, p1, p2], stroke)
            }
            CurveGeometry::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => lower::arc(store, center, radius, start_angle, sweep, stroke),
        }
    }
}
