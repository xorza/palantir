use crate::primitives::Size;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub size: Size,
}

impl Rect {
    pub const ZERO: Self = Self { min: Vec2::ZERO, size: Size::ZERO };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { min: Vec2::new(x, y), size: Size::new(w, h) }
    }

    pub fn max(&self) -> Vec2 { self.min + Vec2::new(self.size.w, self.size.h) }
    pub fn width(&self) -> f32 { self.size.w }
    pub fn height(&self) -> f32 { self.size.h }

    pub fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }
}
