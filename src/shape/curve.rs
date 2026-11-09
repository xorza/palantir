//! [`CurveShape`]: a line or a Bézier, which a [`Stroke`] draws; every geometry lowers to one
//! `ShapeRecord::Curve`.

use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::quadratic_bezier::QuadraticBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::PaintedBy;
use crate::shape::draw::Draw;
use crate::shape::paint::point_placement::PointPlacement;
use crate::shape::sealed;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveGeometry {
    Line { a: Vec2, b: Vec2 },
    CubicBezier(CubicBezier),
    QuadraticBezier(QuadraticBezier),
}

impl CurveGeometry {
    /// The one point a curve of no length sits at; `None` for a curve with length.
    pub(crate) fn point(self) -> Option<Vec2> {
        match self {
            Self::Line { a, b } => vec2::approx_eq(a, b).then_some(a),
            Self::CubicBezier(CubicBezier { p0, p1, p2, p3 }) => {
                (vec2::approx_eq(p0, p1) && vec2::approx_eq(p0, p2) && vec2::approx_eq(p0, p3))
                    .then_some(p0)
            }
            Self::QuadraticBezier(QuadraticBezier { p0, p1, p2 }) => {
                (vec2::approx_eq(p0, p1) && vec2::approx_eq(p0, p2)).then_some(p0)
            }
        }
    }

    /// The cubic every geometry draws as: a line's thirds, a quadratic's degree elevation.
    pub(crate) fn bezier(self) -> CubicBezier {
        match self {
            Self::Line { a, b } => {
                let third = (b - a) / 3.0;
                CubicBezier {
                    p0: a,
                    p1: a + third,
                    p2: b - third,
                    p3: b,
                }
            }
            Self::CubicBezier(bezier) => bezier,
            Self::QuadraticBezier(bezier) => CubicBezier::from(bezier),
        }
    }
}

/// A line or a Bézier, its points logical px from its rect's top-left: the owner's arranged rect
/// or [`Self::at`] a rect of its own, its points fit into it by [`Self::fit`]. A [`Stroke`] draws
/// it ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct CurveShape {
    pub(crate) geometry: CurveGeometry,
    pub(crate) placement: PointPlacement,
}

impl CurveShape {
    pub(super) const fn new(geometry: CurveGeometry) -> Self {
        Self {
            geometry,
            placement: PointPlacement::NATURAL,
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.placement.local_rect = Some(rect);
        self
    }

    /// Fit the points into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement.fit = fit;
        self
    }
}

impl PaintedBy<Stroke> for CurveShape {}

/// A line or a Bézier drawn with a [`Stroke`].
impl sealed::DrawWith<Stroke> for CurveShape {
    #[track_caller]
    fn check(&self, stroke: &Stroke) {
        stroke.validate();
    }

    fn draw<'a>(&'a self, stroke: &'a Stroke) -> Draw<'a> {
        Draw::Curve {
            shape: self,
            stroke,
        }
    }
}
