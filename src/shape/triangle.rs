//! The triangle builder; lowers to `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
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
}

impl TriangleShape {
    /// Interior paint.
    ///
    /// # Panics
    ///
    /// Panics unless `fill` is a [colour](crate::widget::domain::color).
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
    /// the colour a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn border(mut self, border: Stroke) -> Self {
        border.validate();
        self.border = border;
        self
    }

    /// Round all three corners by this radius.
    ///
    /// # Panics
    ///
    /// Panics unless `radius` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn radius(mut self, radius: f32) -> Self {
        self.radius = domain::length(radius);
        self
    }
}

/// Whether the triangle's own area paints nothing. A rounding radius
/// can still paint around it — see [`TriangleShape`]'s `is_noop`.
#[inline]
fn triangle_paint_empty(a: Vec2, b: Vec2, c: Vec2) -> bool {
    let ab = b - a;
    let ac = c - a;
    let bc = c - b;
    let max_edge_len_sq = ab
        .length_squared()
        .max(ac.length_squared())
        .max(bc.length_squared());
    let normalized_twice_area = ab.perp_dot(ac).abs() / max_edge_len_sq;
    is_invisible(normalized_twice_area)
}
impl sealed::LowerShape for TriangleShape {
    /// A thin or collapsed triangle with a radius paints a bar or a disc, so it is not empty.
    fn is_noop(&self) -> bool {
        (self.fill.is_noop() && self.border.is_noop())
            || (is_invisible(self.radius) && triangle_paint_empty(self.a, self.b, self.c))
    }

    /// `radius` must be checked here: lowering launders NaN to `0.0`, so the record would hide it.
    fn has_nan(&self) -> bool {
        self.a.has_nan()
            || self.b.has_nan()
            || self.c.has_nan()
            || self.radius.is_nan()
            || self.fill.has_nan()
            || self.border.has_nan()
    }

    /// `bbox` is the AABB of `a`/`b`/`c` inflated by `radius`. The AA fringe is added later in `cascade::paint_rect`, since it is half a physical pixel and this rect is logical. Nothing is staged, so nothing goes through `lower::`.
    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            a,
            b,
            c,
            radius,
            fill,
            border,
        } = self;
        // Through `Aabb`: raw `min`/`max` would launder a NaN corner out of the bounds.
        let bbox = Aabb::of(&[a, b, c]).inflated(radius.max(0.0));
        ShapeRecord::Quad(QuadShape::Triangle {
            a,
            b,
            c,
            radius,
            fill: fill.into(),
            border: ShapeStroke::from(border),
            bbox,
        })
    }
}
