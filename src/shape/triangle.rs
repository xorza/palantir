//! The triangle builder. Lowers to
//! `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use crate::primitives::rect::aabb::Aabb;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::{QuadShape, ShapeStroke};
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::stroke_bounds::HALF_FRINGE;
use glam::Vec2;

/// Filled and/or stroked triangle with optional uniform corner rounding.
#[derive(Clone, Debug)]
pub struct TriangleShape {
    pub(crate) a: Vec2,
    pub(crate) b: Vec2,
    pub(crate) c: Vec2,
    pub(crate) radius: f32,
    pub(crate) fill: Color,
    pub(crate) stroke: Stroke,
}

shape_setters!(TriangleShape {
    fill: Color => fill,
    stroke: Stroke => stroke,
    radius: f32 => radius,
});

#[inline]
fn triangle_paint_empty(a: Vec2, b: Vec2, c: Vec2) -> bool {
    let ab = b - a;
    let ac = c - a;
    let bc = c - b;
    let max_edge_len_sq = ab
        .length_squared()
        .max(ac.length_squared())
        .max(bc.length_squared());
    // Longest-edge normalization keeps the cutoff independent of authored scale.
    let normalized_twice_area = ab.perp_dot(ac).abs() / max_edge_len_sq;
    noop_f32(normalized_twice_area)
}
impl sealed::LowerShape for TriangleShape {
    fn is_noop(&self) -> bool {
        (self.fill.is_noop() && self.stroke.is_noop())
            // A NaN corner falls out of this for free — the area
            // arithmetic propagates it and `noop_f32` reads NaN as
            // invisible. `radius` gets no such cover, and lowering
            // launders it (`radius.max(0.0)` is `0.0` for NaN), so it
            // has to be named.
            || self.radius.is_nan()
            || triangle_paint_empty(self.a, self.b, self.c)
    }

    /// `bbox` is the owner-local AABB of `a`/`b`/`c` inflated by
    /// `radius + AA fringe` — the SDF offsets the shape outward by
    /// `radius`, and the stroke is inner-edge and adds no outward reach —
    /// so damage and clip-cull cover the rounded, antialiased extent.
    /// Nothing is staged, so nothing goes through `lower::`.
    fn lower(self, _store: &RecordStore) -> ShapeRecord {
        let Self {
            a,
            b,
            c,
            radius,
            fill,
            stroke,
        } = self;
        // Through `Aabb`, not raw `min`/`max`: those launder a NaN corner
        // out of the bounds, which would leave the record-level gate
        // testing a finite bbox for a shape that has one.
        let pad = radius.max(0.0) + HALF_FRINGE;
        let bbox = Aabb::of(&[a, b, c]).inflated(pad);
        ShapeRecord::Quad(QuadShape::Triangle {
            a,
            b,
            c,
            radius,
            fill: fill.into(),
            stroke: ShapeStroke::from(stroke),
            bbox,
        })
    }
}
