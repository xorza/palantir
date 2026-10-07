//! The transform a node can carry: uniform scale plus translation, which
//! layout and hit-test paths can invert exactly.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::{self, is_approx_zero, vec2};
use glam::Vec2;

/// A 2D transform with uniform scale and translation, like
/// `kurbo::TranslateScale`, used for pan/zoom of `Panel` subtrees. Stricter
/// than a full affine (no rotation, skew or non-uniform scale), which keeps
/// axis-aligned rects axis-aligned for scissor and hit-test math, and the
/// rounded-rect SDF shader unchanged.
///
/// Translation is finite and scale positive and finite. Mirroring is excluded:
/// it needs a full affine and min/max handling throughout layout, hit-testing
/// and paint.
///
/// `compose(p) = self(other(p))`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct TranslateScale {
    pub(crate) translation: Vec2,
    pub(crate) scale: f32,
}

impl TranslateScale {
    /// No translation, unit scale.
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        scale: 1.0,
    };

    /// True when this transform won't visibly move or scale descendants.
    ///
    /// Not a paint predicate: it asks "is this ≈ this constant" and gates a
    /// fast path, so a NaN lane reports `false` and avoids the shortcut, where
    /// a paint no-op reports `true` and drops the draw.
    ///
    /// Bitwise equality with `IDENTITY` is the fast path; the fallback treats
    /// sub-`EPS` drift (from animation or lerping) as identity.
    #[inline]
    pub const fn is_identity(self) -> bool {
        if self.translation.x.to_bits() == Self::IDENTITY.translation.x.to_bits()
            && self.translation.y.to_bits() == Self::IDENTITY.translation.y.to_bits()
            && self.scale.to_bits() == Self::IDENTITY.scale.to_bits()
        {
            return true;
        }
        is_approx_zero(self.translation.x)
            && is_approx_zero(self.translation.y)
            && is_approx_zero(self.scale - 1.0)
    }

    /// Construct a validated transform. `translation`: an *offset* per axis;
    /// `scale`: *positive*.
    ///
    /// # Panics
    ///
    /// Panics unless both translation axes are
    /// [offsets](crate::widget::domain::offset) and `scale` is
    /// [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn new(translation: Vec2, scale: f32) -> Self {
        Self {
            translation: vec2::offset(translation),
            scale: domain::positive(scale),
        }
    }

    /// Build from parts already known good. The invariant is closed under the
    /// operations below, short of overflow (two finite scales multiplying to
    /// `inf`), so the checks [`Self::new`] makes in release are debug-only here:
    /// `compose` runs per transformed node per frame in the cascade and again
    /// per shape in the composer, where release must pay only the arithmetic.
    const fn from_parts(translation: Vec2, scale: f32) -> Self {
        debug_assert!(vec2::is_offset(translation), "{}", domain::OFFSET_RULE);
        debug_assert!(domain::is_positive(scale), "{}", domain::POSITIVE_RULE);
        Self { translation, scale }
    }

    /// Move by `t`, at unit scale.
    pub const fn from_translation(t: Vec2) -> Self {
        Self::new(t, 1.0)
    }

    /// Scale by `s` about the origin; see [`Self::anchored_at`] for a pivot.
    pub const fn from_scale(s: f32) -> Self {
        Self::new(Vec2::ZERO, s)
    }

    /// Re-anchor `self` so its scale pivots about `origin` instead of (0, 0):
    ///
    /// ```text
    /// p ↦ (p - origin) * scale + origin + translation
    ///   = p * scale + (origin * (1 - scale) + translation)
    /// ```
    ///
    /// The cascade and encoder apply a node's `Panel::transform` this way:
    /// `layout_rect.min` is in absolute parent-frame coords, so a raw `self`
    /// would scale the node's own origin and drift at non-1.0 scale. At
    /// `scale == 1` the translation is unchanged. It builds `from_parts` as
    /// this is the cascade's per-transformed-node path.
    pub const fn anchored_at(self, origin: Vec2) -> Self {
        Self::from_parts(
            Vec2::new(
                origin.x * (1.0 - self.scale) + self.translation.x,
                origin.y * (1.0 - self.scale) + self.translation.y,
            ),
            self.scale,
        )
    }

    /// Apply `self` after `other`: `result(p) == self.apply_point(other.apply_point(p))`.
    /// Descend the tree with `parent_cumulative.compose(child_local)`. The
    /// invariant is closed under composition, so release builds pay only the
    /// arithmetic (see `from_parts`).
    pub const fn compose(self, other: Self) -> Self {
        Self::from_parts(
            Vec2::new(
                other.translation.x * self.scale + self.translation.x,
                other.translation.y * self.scale + self.translation.y,
            ),
            self.scale * other.scale,
        )
    }

    /// Map a point through this transform. For a direction or offset, which
    /// translation does not apply to, use [`Self::inverse_vector`].
    pub const fn apply_point(self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x * self.scale + self.translation.x,
            p.y * self.scale + self.translation.y,
        )
    }

    /// Undo this transform for a direction or offset (no translation).
    pub const fn inverse_vector(self, v: Vec2) -> Vec2 {
        Vec2::new(v.x / self.scale, v.y / self.scale)
    }

    /// [`Self::apply_point`] for a whole rect: origin and extent.
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
    use crate::internals::panic_probe;
    use crate::primitives::math::domain::EPS;

    /// A transform is the identity when its bits are, or each part is within
    /// `EPS` of it. `-0.0` and lerp drift take the `EPS` fallback; one pixel or
    /// a 1.5 scale is past it.
    #[test]
    fn is_identity_within_eps() {
        let cases = [
            ("IDENTITY", TranslateScale::IDENTITY, true),
            ("new zero, 1", TranslateScale::new(Vec2::ZERO, 1.0), true),
            (
                "negative zero",
                TranslateScale::new(Vec2::new(-0.0, -0.0), 1.0),
                true,
            ),
            (
                "half-EPS drift",
                TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5),
                true,
            ),
            (
                "one pixel",
                TranslateScale::from_translation(Vec2::new(1.0, 0.0)),
                false,
            ),
            ("scale 1.5", TranslateScale::from_scale(1.5), false),
        ];
        for (label, t, identity) in cases {
            assert_eq!(t.is_identity(), identity, "{label}");
        }
    }

    /// The door a caller builds a transform at, screened in every build.
    #[test]
    fn construction_rejects_non_finite_translation_and_non_positive_or_non_finite_scale() {
        let invalid_translations = [
            Vec2::new(f32::NAN, 0.0),
            Vec2::new(f32::INFINITY, 0.0),
            Vec2::new(f32::NEG_INFINITY, 0.0),
            Vec2::new(0.0, f32::NAN),
            Vec2::new(0.0, f32::INFINITY),
            Vec2::new(0.0, f32::NEG_INFINITY),
        ];
        for translation in invalid_translations {
            panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
                TranslateScale::new(translation, 1.0)
            });
        }

        for scale in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
                TranslateScale::new(Vec2::ZERO, scale)
            });
        }
    }

    /// The type's own arithmetic is held to the same contract, in debug only:
    /// `from_parts` is where `compose` lands, per node per frame. Overflow is
    /// how a derived transform breaks.
    #[cfg(debug_assertions)]
    #[test]
    fn derived_transforms_reject_the_overflow_their_arithmetic_produces() {
        const TRANSLATION: &str = domain::OFFSET_RULE;
        const SCALE: &str = domain::POSITIVE_RULE;
        // Pivot arithmetic that overflows translation.
        panic_probe::assert_panics_with(TRANSLATION, || {
            TranslateScale::from_scale(f32::MAX).anchored_at(Vec2::splat(f32::MAX))
        });
        // Composition that overflows scale.
        panic_probe::assert_panics_with(SCALE, || {
            TranslateScale::from_scale(f32::MAX).compose(TranslateScale::from_scale(2.0))
        });
        // Composition that underflows scale to zero.
        panic_probe::assert_panics_with(SCALE, || {
            TranslateScale::from_scale(f32::from_bits(1)).compose(TranslateScale::from_scale(0.5))
        });
        // Composition that overflows translation.
        panic_probe::assert_panics_with(TRANSLATION, || {
            let transform = TranslateScale::from_translation(Vec2::splat(f32::MAX));
            transform.compose(transform)
        });
    }

    #[test]
    fn composition_rect_application_and_inverse_vector_agree_exactly() {
        let parent = TranslateScale::new(Vec2::new(3.0, 5.0), 2.0);
        let child = TranslateScale::new(Vec2::new(7.0, 11.0), 4.0);
        let composed = parent.compose(child);

        assert_eq!(composed.translation, Vec2::new(17.0, 27.0));
        assert_eq!(composed.scale, 8.0);
        let point = Vec2::new(2.0, 3.0);
        assert_eq!(
            composed.apply_point(point),
            parent.apply_point(child.apply_point(point))
        );
        assert_eq!(
            composed.apply_rect(Rect::new(-2.0, 3.0, 4.0, 5.0)),
            Rect::new(1.0, 51.0, 32.0, 40.0)
        );
        assert_eq!(
            composed.inverse_vector(Vec2::new(24.0, -40.0)),
            Vec2::new(3.0, -5.0)
        );
    }
}
