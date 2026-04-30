#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Spacing {
    pub left: f32, pub top: f32, pub right: f32, pub bottom: f32,
}

impl Spacing {
    pub const ZERO: Self = Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
    pub const fn all(v: f32) -> Self { Self { left: v, top: v, right: v, bottom: v } }
    pub fn horiz(&self) -> f32 { self.left + self.right }
    pub fn vert(&self) -> f32 { self.top + self.bottom }
}
