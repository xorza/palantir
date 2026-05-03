use crate::primitives::{Rect, Size};
use glam::Vec2;

/// A 2D transform with uniform scale and translation — same shape as
/// `kurbo::TranslateScale`. Used for pan/zoom of `Panel` subtrees. Stricter
/// than a full affine (no rotation/skew/non-uniform scale), which keeps:
/// - axis-aligned rects axis-aligned, so scissor and hit-test math stay simple,
/// - the rounded-rect SDF shader unchanged (CPU-side parameter scaling only).
///
/// Apply `self` after `other` via `compose`: `compose(p) = self(other(p))`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TranslateScale {
    pub translation: Vec2,
    pub scale: f32,
}

impl TranslateScale {
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        scale: 1.0,
    };

    pub const fn new(translation: Vec2, scale: f32) -> Self {
        Self { translation, scale }
    }

    pub const fn from_translation(t: Vec2) -> Self {
        Self {
            translation: t,
            scale: 1.0,
        }
    }

    pub const fn from_scale(s: f32) -> Self {
        Self {
            translation: Vec2::ZERO,
            scale: s,
        }
    }

    /// Apply `self` after `other`: `result(p) == self.apply_point(other.apply_point(p))`.
    /// Matches matrix multiplication conventions — descend the tree by composing
    /// `parent_cumulative.compose(child_local)`.
    pub const fn compose(self, other: Self) -> Self {
        Self {
            scale: self.scale * other.scale,
            translation: Vec2::new(
                other.translation.x * self.scale + self.translation.x,
                other.translation.y * self.scale + self.translation.y,
            ),
        }
    }

    pub const fn apply_point(self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x * self.scale + self.translation.x,
            p.y * self.scale + self.translation.y,
        )
    }

    pub const fn apply_rect(self, r: Rect) -> Rect {
        Rect {
            min: Vec2::new(
                r.min.x * self.scale + self.translation.x,
                r.min.y * self.scale + self.translation.y,
            ),
            size: Size::new(r.size.w * self.scale, r.size.h * self.scale),
        }
    }
}

impl Default for TranslateScale {
    fn default() -> Self {
        Self::IDENTITY
    }
}
