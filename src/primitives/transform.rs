use crate::primitives::{Rect, Size};
use glam::Vec2;

/// A 2D transform with uniform scale and translation — same shape as
/// `kurbo::TranslateScale`. Used for pan/zoom of `Panel` subtrees. Stricter
/// than a full affine (no rotation/skew/non-uniform scale), which keeps:
/// - axis-aligned rects axis-aligned, so scissor and hit-test math stay simple,
/// - the rounded-rect SDF shader unchanged (CPU-side parameter scaling only).
///
/// Apply `self` after `other` via `compose`: `compose(p) = self(other(p))`.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub fn compose(self, other: Self) -> Self {
        Self {
            scale: self.scale * other.scale,
            translation: other.translation * self.scale + self.translation,
        }
    }

    pub fn apply_point(self, p: Vec2) -> Vec2 {
        p * self.scale + self.translation
    }

    pub fn apply_rect(self, r: Rect) -> Rect {
        Rect {
            min: r.min * self.scale + self.translation,
            size: Size::new(r.size.w * self.scale, r.size.h * self.scale),
        }
    }
}

impl Default for TranslateScale {
    fn default() -> Self {
        Self::IDENTITY
    }
}
