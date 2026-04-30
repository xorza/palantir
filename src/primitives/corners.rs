use crate::primitives::Size;
use glam::Vec2;

/// Per-corner radii. `Vec2`/`Size` map to (top, bottom) pairs; `f32` is uniform.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl Corners {
    pub const ZERO: Self = Self { tl: 0.0, tr: 0.0, br: 0.0, bl: 0.0 };
    pub const fn all(r: f32) -> Self { Self { tl: r, tr: r, br: r, bl: r } }
    pub const fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self { Self { tl, tr, br, bl } }
}

impl From<f32> for Corners {
    fn from(r: f32) -> Self { Self::all(r) }
}

impl From<Vec2> for Corners {
    fn from(v: Vec2) -> Self { Self { tl: v.x, tr: v.x, br: v.y, bl: v.y } }
}

impl From<Size> for Corners {
    fn from(s: Size) -> Self { Self { tl: s.w, tr: s.w, br: s.h, bl: s.h } }
}
