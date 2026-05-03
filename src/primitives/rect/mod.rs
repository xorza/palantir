#[cfg(test)]
mod tests;

use crate::primitives::{Size, Spacing};
use glam::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
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

    pub const fn max(&self) -> Vec2 {
        Vec2::new(self.min.x + self.size.w, self.min.y + self.size.h)
    }
    pub const fn width(&self) -> f32 {
        self.size.w
    }
    pub const fn height(&self) -> f32 {
        self.size.h
    }
    pub const fn area(&self) -> f32 {
        self.size.w * self.size.h
    }

    pub const fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// Inset by `s` on each side, clamping the resulting size at zero. Used for
    /// margin / padding insets in the layout pass.
    pub const fn deflated_by(&self, s: Spacing) -> Self {
        let w = self.size.w - s.horiz();
        let h = self.size.h - s.vert();
        Self {
            min: Vec2::new(self.min.x + s.left, self.min.y + s.top),
            size: Size::new(if w < 0.0 { 0.0 } else { w }, if h < 0.0 { 0.0 } else { h }),
        }
    }

    /// True if `self` and `other` overlap on both axes (strict — touching
    /// edges don't count). Used by the encoder's damage-rect filter to
    /// decide whether a node's paint commands can be skipped.
    pub const fn intersects(&self, other: Self) -> bool {
        let a_max = self.max();
        let b_max = other.max();
        self.min.x < b_max.x
            && other.min.x < a_max.x
            && self.min.y < b_max.y
            && other.min.y < a_max.y
    }

    /// Axis-aligned intersection. Returns a zero-size rect if the inputs
    /// don't overlap (either dimension goes negative).
    pub const fn intersect(&self, other: Self) -> Self {
        let min_x = if self.min.x > other.min.x {
            self.min.x
        } else {
            other.min.x
        };
        let min_y = if self.min.y > other.min.y {
            self.min.y
        } else {
            other.min.y
        };
        let a_max_x = self.min.x + self.size.w;
        let b_max_x = other.min.x + other.size.w;
        let max_x = if a_max_x < b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.min.y + self.size.h;
        let b_max_y = other.min.y + other.size.h;
        let max_y = if a_max_y < b_max_y { a_max_y } else { b_max_y };
        let w = max_x - min_x;
        let h = max_y - min_y;
        Self {
            min: Vec2::new(min_x, min_y),
            size: Size::new(if w < 0.0 { 0.0 } else { w }, if h < 0.0 { 0.0 } else { h }),
        }
    }

    /// Smallest axis-aligned rect enclosing both `self` and `other`. Used by
    /// damage-rect computation to union prev+curr rects of dirty nodes.
    /// A zero-sized rect at the origin (the `Default`) acts as the identity
    /// for accumulation: `Rect::ZERO.union(r) == r` only when `r`'s min is
    /// `(0,0)` — callers should fold over `Option<Rect>` to avoid biasing
    /// toward the origin.
    pub const fn union(&self, other: Self) -> Self {
        let min_x = if self.min.x < other.min.x {
            self.min.x
        } else {
            other.min.x
        };
        let min_y = if self.min.y < other.min.y {
            self.min.y
        } else {
            other.min.y
        };
        let a_max_x = self.min.x + self.size.w;
        let b_max_x = other.min.x + other.size.w;
        let max_x = if a_max_x > b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.min.y + self.size.h;
        let b_max_y = other.min.y + other.size.h;
        let max_y = if a_max_y > b_max_y { a_max_y } else { b_max_y };
        Self {
            min: Vec2::new(min_x, min_y),
            size: Size::new(max_x - min_x, max_y - min_y),
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
