//! [`TriangleShape`]: a triangle with rounded corners; lowers to `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::paint::Paint;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::point_placement::PointPlacement;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::triangle_geometry::{self, TriangleGeometry};
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::{self, IntoPaint, Resolve};
use glam::Vec2;

/// A triangle, its corners logical px from its rect's top-left, with optional uniform corner
/// rounding: the owner's arranged rect or [`Self::at`] a rect of its own, its corners fit into it
/// by [`Self::fit`]. A [`Paint`] of a solid fill and a border covers it
/// ([`Ui::add_shape`](crate::Ui::add_shape)): the quad lanes it packs into hold no gradient.
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
}

/// A triangle of `P`: its solid fill and its border.
impl<P: IntoPaint> sealed::LowerShape for (TriangleShape, P) {
    /// # Panics
    ///
    /// Unless the paint holds its kinds, its fill is solid, and it has no shadow or material,
    /// which a triangle draws none of.
    #[track_caller]
    fn validate(&self) {
        let paint: Paint = self.1.clone().into();
        paint.validate();
        assert!(
            matches!(paint.fill, Brush::Solid(_)),
            "a triangle's fill is solid: {:?}",
            paint.fill
        );
        assert!(paint.shadow.is_noop(), "a triangle casts no shadow");
        assert!(paint.material.is_none(), "a triangle wears no material");
    }

    /// The corners keep their placement, which resolves where the triangle is drawn
    /// ([`TriangleGeometry::place`]); a triangle of no area drops here, as no fit gives it one. A
    /// rect of no size keeps it: `Natural` only moves the corners by the rect's corner, and a
    /// scaling fit collapses it where it is placed.
    fn lower(self, _resolve: Resolve<'_>, _store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            TriangleShape {
                corners,
                radius,
                placement,
            },
            paint,
        ) = self;
        let paint: Paint = paint.into();
        let Brush::Solid(fill) = paint.fill else {
            unreachable!("`validate` holds a triangle's fill solid");
        };
        if (fill.is_noop() && paint.border.is_noop())
            || triangle_geometry::area_is_paint_empty(corners)
        {
            return None;
        }
        Some(ShapeRecord::Quad(QuadShape::Triangle {
            geometry: TriangleGeometry {
                corners,
                radius,
                placement,
            },
            fill: fill.to_f16(),
            border: LoweredBorder::lower(paint.border),
        }))
    }
}
