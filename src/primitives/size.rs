#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    pub const INF: Self = Self { w: f32::INFINITY, h: f32::INFINITY };

    pub const fn new(w: f32, h: f32) -> Self { Self { w, h } }

    pub fn min(self, other: Self) -> Self {
        Self { w: self.w.min(other.w), h: self.h.min(other.h) }
    }
    pub fn max(self, other: Self) -> Self {
        Self { w: self.w.max(other.w), h: self.h.max(other.h) }
    }
}
