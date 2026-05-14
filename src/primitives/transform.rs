use crate::primitives::{rect::Rect, size::Size};
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

    /// True when this transform won't visibly move/scale descendants.
    /// Two-stage check:
    /// - Fast path: 12-byte equality with `IDENTITY` — a single
    ///   `memcmp` on the `#[repr(C)] Pod` layout, faster than three
    ///   f32 `feq` instructions.
    /// - Approx fallback (only when the fast path misses): treats
    ///   sub-`EPS` numerical drift as identity. Catches transforms
    ///   that animation/lerping produced bit-different from
    ///   `IDENTITY` but visually indistinguishable.
    #[inline]
    pub fn is_noop(&self) -> bool {
        let s: [u32; 3] = bytemuck::cast(*self);
        let id: [u32; 3] = bytemuck::cast(Self::IDENTITY);
        if s == id {
            return true;
        }
        crate::primitives::approx::approx_zero(self.translation.x)
            && crate::primitives::approx::approx_zero(self.translation.y)
            && crate::primitives::approx::approx_zero(self.scale - 1.0)
    }

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

    /// Scale by `s` about the pivot `center` (in the *parent* coordinate
    /// space the transform is applied in). The pivot is folded into the
    /// translation at construction time:
    ///
    /// ```text
    /// p ↦ (p - center) * s + center
    ///   = p * s + center * (1 - s)
    /// ```
    ///
    /// so the runtime representation stays the same uniform-scale + translate
    /// pair. Useful for "scale about my own center" / "zoom toward cursor"
    /// effects where origin-relative scaling would translate the content away
    /// from where the user expects.
    pub const fn from_scale_about(center: Vec2, s: f32) -> Self {
        Self {
            translation: Vec2::new(center.x * (1.0 - s), center.y * (1.0 - s)),
            scale: s,
        }
    }

    /// Scale by `s` about `center`, then translate by `translation`. The
    /// pivot and the additional translation collapse into the single
    /// `translation` field at construction:
    ///
    /// ```text
    /// p ↦ (p - center) * s + center + translation
    ///   = p * s + center * (1 - s) + translation
    /// ```
    ///
    /// so the runtime representation stays a plain uniform-scale +
    /// translate pair — same compose/apply paths, no extra fields.
    /// Useful when an animation wants both a pan and a pivot-anchored
    /// zoom in one step (e.g. "zoom toward cursor while easing the
    /// content into view").
    pub const fn from_translate_scale_about(translation: Vec2, center: Vec2, s: f32) -> Self {
        Self {
            translation: Vec2::new(
                center.x * (1.0 - s) + translation.x,
                center.y * (1.0 - s) + translation.y,
            ),
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
