use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use crate::primitives::stroke::Stroke;
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

impl TriangleShape {
    pub fn fill(mut self, fill: impl Into<Color>) -> Self {
        self.fill = fill.into();
        self
    }

    pub fn stroke(mut self, stroke: impl Into<Stroke>) -> Self {
        self.stroke = stroke.into();
        self
    }

    pub fn radius(mut self, radius: impl Into<f32>) -> Self {
        self.radius = radius.into();
        self
    }

    pub(super) fn is_noop(&self) -> bool {
        (self.fill.is_noop() && self.stroke.is_noop())
            // A NaN corner falls out of this for free — the area
            // arithmetic propagates it and `noop_f32` reads NaN as
            // invisible. `radius` gets no such cover, and lowering
            // launders it (`radius.max(0.0)` is `0.0` for NaN), so it
            // has to be named.
            || self.radius.is_nan()
            || triangle_paint_empty(self.a, self.b, self.c)
    }
}

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
