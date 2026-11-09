//! [`TriangleShape`]: a triangle with rounded corners; lowers to `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::shape::draw::Draw;
use crate::shape::geometry::point_placement::PointPlacement;
use crate::shape::sealed;
use crate::shape::{IntoPaint, PaintedBy};
use glam::Vec2;

/// A triangle, its corners logical px from its rect's top-left, with optional uniform corner
/// rounding: the owner's arranged rect or [`Self::at`] a rect of its own, its corners fit into it
/// by [`Self::fit`]. A [`Paint`](crate::Paint) of a fill, a border and a material covers it
/// ([`Ui::add_shape`](crate::Ui::add_shape)), or an [`ImagePaint`](crate::widget::ImagePaint); a
/// gradient fill spans the bounding box of the rounded triangle, as SVG's `objectBoundingBox`
/// spans a shape's, and an image fits it.
#[derive(Clone, Debug)]
#[must_use]
pub struct TriangleShape {
    pub(crate) corners: [Vec2; 3],
    pub(crate) radius: f32,
    pub(crate) placement: PointPlacement,
}

impl TriangleShape {
    pub(super) const fn new(a: Vec2, b: Vec2, c: Vec2) -> Self {
        Self {
            corners: [a, b, c],
            radius: 0.0,
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

    /// Fit the corners into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement.fit = fit;
        self
    }

    /// Round all three corners by this radius. The edges stay on the sharp triangle's, so the
    /// shape only loses its corners; a radius past the inradius paints the incircle.
    ///
    /// # Panics
    ///
    /// Panics unless `radius` is a [length](crate::widget::domain::length) and a
    /// [lane](crate::widget::domain::lane), as it packs into an f16 lane.
    #[track_caller]
    pub const fn radius(mut self, radius: f32) -> Self {
        self.radius = domain::lane(domain::length(radius));
        self
    }
}

impl<P: IntoPaint> PaintedBy<P> for TriangleShape {}

/// A triangle of `P`: its fill, its border and its material.
impl<P: IntoPaint> sealed::DrawWith<P> for TriangleShape {
    /// # Panics
    ///
    /// Unless the paint holds its kinds and has no shadow, which a triangle casts none of.
    #[track_caller]
    fn check(&self, paint: &P) {
        let paint = paint.to_paint();
        paint.validate();
        assert!(paint.shadow.is_noop(), "a triangle casts no shadow");
    }

    fn draw<'a>(&'a self, paint: &'a P) -> Draw<'a> {
        Draw::Triangle {
            shape: self,
            paint: paint.to_paint(),
        }
    }
}
