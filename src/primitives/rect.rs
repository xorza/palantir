use crate::primitives::{Size, Spacing};
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub size: Size,
}

impl Rect {
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        size: Size::ZERO,
    };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            size: Size::new(w, h),
        }
    }

    pub fn max(&self) -> Vec2 {
        self.min + Vec2::new(self.size.w, self.size.h)
    }
    pub fn width(&self) -> f32 {
        self.size.w
    }
    pub fn height(&self) -> f32 {
        self.size.h
    }

    pub fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// Inset by `s` on each side, clamping the resulting size at zero. Used for
    /// margin / padding insets in the layout pass.
    pub fn deflated_by(&self, s: Spacing) -> Self {
        Self {
            min: self.min + Vec2::new(s.left, s.top),
            size: Size::new(
                (self.size.w - s.horiz()).max(0.0),
                (self.size.h - s.vert()).max(0.0),
            ),
        }
    }

    /// Axis-aligned intersection. Returns a zero-size rect if the inputs
    /// don't overlap (either dimension goes negative).
    pub fn intersect(&self, other: Self) -> Self {
        let min_x = self.min.x.max(other.min.x);
        let min_y = self.min.y.max(other.min.y);
        let max_x = (self.min.x + self.size.w).min(other.min.x + other.size.w);
        let max_y = (self.min.y + self.size.h).min(other.min.y + other.size.h);
        Self {
            min: Vec2::new(min_x, min_y),
            size: Size::new((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)),
        }
    }

    /// Scale by `scale` and optionally snap edges to integer pixels. Used at
    /// the logical→physical-px boundary inside the renderer; snapping derives
    /// width/height from rounded edges (not from `size * scale`) to avoid
    /// creeping width drift across rows of identical rects.
    pub fn scaled_by(&self, scale: f32, snap: bool) -> Self {
        let mut left = self.min.x * scale;
        let mut top = self.min.y * scale;
        let mut right = (self.min.x + self.size.w) * scale;
        let mut bottom = (self.min.y + self.size.h) * scale;
        if snap {
            left = left.round();
            top = top.round();
            right = right.round();
            bottom = bottom.round();
        }
        Self {
            min: Vec2::new(left, top),
            size: Size::new((right - left).max(0.0), (bottom - top).max(0.0)),
        }
    }
}
