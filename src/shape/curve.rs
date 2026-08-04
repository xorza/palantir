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

/// Stroked line, Bézier, or circular arc.
#[derive(Clone, Debug)]
pub struct CurveShape {
    pub(crate) geometry: CurveGeometry,
    pub(crate) width: f32,
    pub(crate) brush: CurveBrush,
    pub(crate) cap: LineCap,
}

impl CurveShape {
    pub(super) fn new(geometry: CurveGeometry, width: f32) -> Self {
        Self {
            geometry,
            width,
            brush: CurveBrush::TRANSPARENT,
            cap: LineCap::Butt,
        }
    }

    pub fn brush(mut self, brush: impl Into<CurveBrush>) -> Self {
        self.brush = brush.into();
        self
    }

    pub fn cap(mut self, cap: impl Into<LineCap>) -> Self {
        self.cap = cap.into();
        self
    }

    pub(crate) fn is_noop(&self) -> bool {
        if noop_f32(self.width) || self.brush.is_noop() {
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
}
// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for CurveShape {
    fn is_noop(&self) -> bool {
        CurveShape::is_noop(self)
    }

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        match self.geometry {
            CurveGeometry::Line { a, b } => {
                lower::line(store, a, b, self.width, self.brush, self.cap)
            }
            CurveGeometry::CubicBezier { p0, p1, p2, p3 } => {
                lower::cubic_bezier(store, [p0, p1, p2, p3], self.width, self.brush, self.cap)
            }
            CurveGeometry::QuadraticBezier { p0, p1, p2 } => {
                lower::quadratic_bezier(store, [p0, p1, p2], self.width, self.brush, self.cap)
            }
            CurveGeometry::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => lower::arc(
                store,
                center,
                radius,
                start_angle,
                sweep,
                self.width,
                self.brush,
                self.cap,
            ),
        }
    }
}
