//! The triangle builder. Lowers to
//! `ShapeRecord::Quad(QuadShape::Triangle)`.

use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::aabb::Aabb;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::{QuadShape, ShapeStroke};
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
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
            || triangle_paint_empty(self.a, self.b, self.c)
    }

    /// `radius` has to be named. Lowering launders it —
    /// `radius.max(0.0)` is `0.0` for NaN — so the record carries no
    /// trace of it, and a NaN corner would reach the SDF as a
    /// sharp-cornered triangle whose bbox was inflated by nothing.
    fn has_nan(&self) -> bool {
        self.a.has_nan()
            || self.b.has_nan()
            || self.c.has_nan()
            || self.radius.is_nan()
            || self.fill.has_nan()
            || self.stroke.has_nan()
    }

    /// `bbox` is the owner-local AABB of `a`/`b`/`c` inflated by
    /// `radius`: the SDF offsets the shape outward by that much to round
    /// its corners, and the stroke is inner-edge and adds no outward
    /// reach.
    ///
    /// The AA fringe is **not** folded in here. It is half a *physical*
    /// pixel, and this rect is owner-local logical px — baking it in
    /// under-covers below scale 1 and over-covers above. Every stroked
    /// kind adds it in `cascade::paint_rect`, after lifting to screen
    /// space where the display scale is in hand.
    ///
    /// Nothing is staged, so nothing goes through `lower::`.
    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            a,
            b,
            c,
            radius,
            fill,
            stroke,
        } = self;
        // Through `Aabb`, not raw `min`/`max`: those launder a NaN corner
        // out of the bounds, which would leave the record's own bbox
        // reading finite for a shape that carries a NaN — and that bbox
        // is what damage and clip-cull are computed from.
        let bbox = Aabb::of(&[a, b, c]).inflated(radius.max(0.0));
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
