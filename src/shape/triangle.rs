//! [`TriangleShape`]: a triangle with rounded corners; lowers to a `ShapeRecord::Closed` triangle.

use crate::primitives::effect::effect_kind::{ColorRead, ErasedRead};
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint::Paint;
use crate::shape::PaintedBy;
use crate::shape::closed_shape::ClosedShape;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::geometry::point_placement::PointPlacement;
use crate::shape::geometry::triangle_geometry::TriangleGeometry;
use glam::Vec2;

/// A triangle, its corners logical px from its rect's top-left, with optional uniform corner
/// rounding: the owner's arranged rect or [`Self::at`] a rect of its own, its corners fit into it
/// by [`Self::fit`]. A [`Paint`] of a fill and a border covers it, or a [`MaterialPaint`]
/// ([`Ui::add_shape`](crate::Ui::add_shape)), or an [`ImagePaint`](crate::ImagePaint); a
/// gradient fill spans the bounding box of the rounded triangle, as SVG's `objectBoundingBox`
/// spans a shape's, and an image fits it.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct TriangleShape {
    /// The rounded triangle this shape lays out.
    pub(crate) geometry: TriangleGeometry,
}

impl TriangleShape {
    pub(super) const fn new(a: Vec2, b: Vec2, c: Vec2) -> Self {
        Self {
            geometry: TriangleGeometry {
                corners: [a, b, c],
                radius: 0.0,
                placement: PointPlacement::NATURAL,
            },
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::is_offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.geometry.placement.local_rect = Some(rect);
        self
    }

    /// Fit the corners into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.geometry.placement.fit = fit;
        self
    }

    /// Round all three corners by this radius. The edges stay on the sharp triangle's, so the
    /// shape only loses its corners; a radius past the inradius paints the incircle.
    ///
    /// # Panics
    ///
    /// Panics unless `radius` is a [length](crate::widget::domain::is_length) and a
    /// [lane](crate::widget::domain::is_lane), as it packs into an f16 lane.
    #[track_caller]
    pub const fn radius(mut self, radius: f32) -> Self {
        self.geometry.radius = domain::lane(domain::length(radius));
        self
    }
}

/// A triangle of `P`: its fill and its border. [`Ui::add_shape`](crate::Ui::add_shape) panics on
/// a shadow, which a triangle casts none of.
impl<P: Into<Paint>> PaintedBy<P> for TriangleShape {}

impl ClosedShape for TriangleShape {
    fn closed_geometry(&self) -> ClosedGeometry {
        ClosedGeometry::Triangle(self.geometry)
    }

    #[track_caller]
    fn check_fill(&self, paint: &Paint) {
        assert!(paint.shadow.is_noop(), "a triangle casts no shadow");
    }

    #[track_caller]
    fn check_effect(&self, read: ErasedRead<'_>) {
        assert!(
            !matches!(read, ErasedRead::Color(ColorRead { outside: Some(_) })),
            "a triangle's effect has no halo"
        );
    }
}

/// A triangle of `MaterialPaint`: its fill and its border over its material, with no shadow, as
/// `P`'s.
impl PaintedBy<MaterialPaint> for TriangleShape {}

/// A triangle of a material alone, as [`MaterialPaint`] draws it.
impl PaintedBy<Material> for TriangleShape {}
