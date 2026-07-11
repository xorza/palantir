use crate::primitives::{approx::approx_zero, rect::Rect, size::Size};
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
    /// - Fast path: bitwise equality with `IDENTITY` via `to_bits`,
    ///   faster than three f32 `feq` instructions.
    /// - Approx fallback (only when the fast path misses): treats
    ///   sub-`EPS` numerical drift as identity. Catches transforms
    ///   that animation/lerping produced bit-different from
    ///   `IDENTITY` but visually indistinguishable.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        if self.translation.x.to_bits() == Self::IDENTITY.translation.x.to_bits()
            && self.translation.y.to_bits() == Self::IDENTITY.translation.y.to_bits()
            && self.scale.to_bits() == Self::IDENTITY.scale.to_bits()
        {
            return true;
        }
        approx_zero(self.translation.x)
            && approx_zero(self.translation.y)
            && approx_zero(self.scale - 1.0)
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

    /// Re-anchor `self` so its scale pivots about `origin` instead of
    /// the cascade's (0, 0). Returns:
    ///
    /// ```text
    /// p ↦ (p - origin) * scale + origin + translation
    ///   = p * scale + (origin * (1 - scale) + translation)
    /// ```
    ///
    /// Used by the cascade/encoder when applying a node's own
    /// `Panel::transform` to its descendants and direct shapes:
    /// `child.layout_rect.min` is in *absolute parent-frame coords*
    /// (post-arrange), so a raw `self` would multiply the transformed
    /// node's own origin too — visible content drift at non-1.0
    /// scale. Anchoring at the node's `layout_rect.min` cancels that
    /// drift, matching the intuitive "scale my body about my own
    /// origin" intent.
    ///
    /// Identity-preserving: when `scale == 1`, `origin * (1 - scale)
    /// == 0` so the translation is unchanged.
    pub const fn anchored_at(self, origin: Vec2) -> Self {
        Self::from_translate_scale_about(self.translation, origin, self.scale)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::approx::EPS;

    #[test]
    fn identity_is_noop_via_fast_path() {
        assert!(TranslateScale::IDENTITY.is_noop());
        assert!(TranslateScale::new(Vec2::ZERO, 1.0).is_noop());
    }

    #[test]
    fn negative_zero_translation_is_noop_via_fallback() {
        // `-0.0.to_bits() != 0.0.to_bits()`, so this misses the bitwise
        // fast path and must fall through to `approx_zero`.
        let t = TranslateScale::new(Vec2::new(-0.0, -0.0), 1.0);
        assert_ne!(t.translation.x.to_bits(), 0.0f32.to_bits());
        assert!(t.is_noop());
    }

    #[test]
    fn sub_eps_drift_is_noop_via_fallback() {
        let t = TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5);
        assert!(t.is_noop());
    }

    #[test]
    fn visible_translation_or_scale_is_not_noop() {
        assert!(!TranslateScale::from_translation(Vec2::new(1.0, 0.0)).is_noop());
        assert!(!TranslateScale::from_scale(1.5).is_noop());
    }
}
