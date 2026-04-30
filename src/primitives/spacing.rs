#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Spacing {
    pub left: f32, pub top: f32, pub right: f32, pub bottom: f32,
}

impl Spacing {
    pub const ZERO: Self = Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
    pub const fn all(v: f32) -> Self { Self { left: v, top: v, right: v, bottom: v } }
    pub const fn xy(x: f32, y: f32) -> Self { Self { left: x, top: y, right: x, bottom: y } }
    pub fn horiz(&self) -> f32 { self.left + self.right }
    pub fn vert(&self) -> f32 { self.top + self.bottom }
}

impl From<f32> for Spacing {
    fn from(v: f32) -> Self { Self::all(v) }
}

/// `(horizontal, vertical)` — both sides on each axis.
impl From<(f32, f32)> for Spacing {
    fn from((x, y): (f32, f32)) -> Self { Self::xy(x, y) }
}

/// `(left, top, right, bottom)` — matches struct field order.
impl From<(f32, f32, f32, f32)> for Spacing {
    fn from((l, t, r, b): (f32, f32, f32, f32)) -> Self {
        Self { left: l, top: t, right: r, bottom: b }
    }
}
