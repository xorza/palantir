//! The triangle builder; lowers to `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;
use glam::Vec2;

/// Filled and/or bordered triangle with optional uniform corner rounding.
#[derive(Clone, Debug)]
#[must_use]
pub struct TriangleShape {
    pub(crate) a: Vec2,
    pub(crate) b: Vec2,
    pub(crate) c: Vec2,
    pub(crate) radius: f32,
    pub(crate) fill: RgbaF32,
    pub(crate) border: Stroke,
}

impl TriangleShape {
    pub(super) const fn new(a: Vec2, b: Vec2, c: Vec2) -> Self {
        Self {
            a,
            b,
            c,
            radius: 0.0,
            fill: RgbaF32::TRANSPARENT,
            border: Stroke::NONE,
        }
    }

    /// Interior paint.
    ///
    /// # Panics
    ///
    /// Panics unless `fill` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn fill(mut self, fill: RgbaF32) -> Self {
        self.fill = domain::color(fill);
        self
    }

    /// Edge paint, inside the boundary like every area shape's.
    ///
    /// # Panics
    ///
    /// Panics unless the width is a [length](crate::widget::domain::length) and
    /// the color a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn border(mut self, border: Stroke) -> Self {
        border.validate();
        self.border = border;
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

    /// Whether the triangle's area paints nothing; rounding stays inside it.
    fn area_is_paint_empty(&self) -> bool {
        let ab = self.b - self.a;
        let ac = self.c - self.a;
        let bc = self.c - self.b;
        let max_edge_len_sq = ab
            .length_squared()
            .max(ac.length_squared())
            .max(bc.length_squared());
        let normalized_twice_area = ab.perp_dot(ac).abs() / max_edge_len_sq;
        is_invisible(normalized_twice_area)
    }
}

impl sealed::LowerShape for TriangleShape {
    fn is_noop(&self) -> bool {
        (self.fill.is_noop() && self.border.is_noop()) || self.area_is_paint_empty()
    }

    /// `bbox` is the rounded shape's own: the corner centers' AABB inflated by the clamped
    /// radius. The AA fringe is added later in `cascade::paint_rect`, since it is half a
    /// physical pixel and this rect is logical.
    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            a,
            b,
            c,
            radius,
            fill,
            border,
        } = self;
        let CornerCenters {
            points: [a, b, c],
            radius,
        } = CornerCenters::new([a, b, c], radius);
        // Through `Aabb`: raw `min`/`max` would launder a NaN corner out of the bounds.
        let bbox = Aabb::of(&[a, b, c]).inflated(radius);
        ShapeRecord::Quad(QuadShape::Triangle {
            a,
            b,
            c,
            radius,
            fill: fill.to_f16(),
            border: ShapeStroke::lower(border),
            bbox,
        })
    }
}

/// The triangle whose dilation by `radius` rounds a sharp one's corners and keeps its edges.
///
/// Each edge moves in by the radius. Every edge line lies one inradius from the incenter, so the
/// move is the homothety about the incenter of ratio `1 - radius / inradius`, the inset Inigo
/// Quilez's rounded box takes before it subtracts the radius
/// (<https://iquilezles.org/articles/distfunctions2d/>). The radius clamps to the inradius,
/// where the centers meet at the incenter and the dilation is the incircle.
#[derive(Debug)]
struct CornerCenters {
    points: [Vec2; 3],
    radius: f32,
}

impl CornerCenters {
    fn new(corners: [Vec2; 3], radius: f32) -> Self {
        let [a, b, c] = corners;
        let side_a = b.distance(c);
        let side_b = c.distance(a);
        let side_c = a.distance(b);
        let perimeter = side_a + side_b + side_c;
        let incenter = (a * side_a + b * side_b + c * side_c) / perimeter;
        let inradius = (b - a).perp_dot(c - a).abs() / perimeter;
        debug_assert!(inradius > 0.0, "`is_noop` refuses a triangle with no area");
        let radius = radius.min(inradius);
        // `v + (incenter - v) * t` rather than a lerp from the incenter: at no radius it returns
        // each corner bit for bit.
        let t = radius / inradius;
        Self {
            points: corners.map(|v| v + (incenter - v) * t),
            radius,
        }
    }
}
