//! The transform a node can carry: uniform scale plus translation, which
//! is every transform the layout and hit-test paths can invert exactly.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::{self, is_approx_zero, vec2};
use glam::Vec2;

/// A 2D transform with uniform scale and translation — same shape as
/// `kurbo::TranslateScale`. Used for pan/zoom of `Panel` subtrees. Stricter
/// than a full affine (no rotation/skew/non-uniform scale), which keeps:
/// - axis-aligned rects axis-aligned, so scissor and hit-test math stay simple,
/// - the rounded-rect SDF shader unchanged (CPU-side parameter scaling only).
///
/// Translation is always finite and scale is always positive and finite.
/// Mirroring is deliberately excluded: it requires a full affine transform
/// and canonical min/max handling throughout layout, hit-testing, and paint.
///
/// Apply `self` after `other` via `compose`: `compose(p) = self(other(p))`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct TranslateScale {
    pub(crate) translation: Vec2,
    pub(crate) scale: f32,
}

impl TranslateScale {
    /// No translation, unit scale — the transform a node sits under
    /// when nothing above it moves or scales.
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        scale: 1.0,
    };

    /// True when this transform won't visibly move/scale descendants.
    ///
    /// **Not a paint predicate**, despite gating draws the way one does:
    /// it asks "is this value ≈ this constant", the question
    /// `is_approx_zero` asks, and it gates a *fast path*. A NaN lane must
    /// therefore report `false` and route around the shortcut, where a
    /// paint no-op reports `true` and drops the draw.
    /// Two-stage check:
    /// - Fast path: bitwise equality with `IDENTITY` via `to_bits`,
    ///   faster than three f32 `feq` instructions.
    /// - Approx fallback (only when the fast path misses): treats
    ///   sub-`EPS` numerical drift as identity. Catches transforms
    ///   that animation/lerping produced bit-different from
    ///   `IDENTITY` but visually indistinguishable.
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

    /// Construct a validated transform. `translation`: an *offset* on each
    /// axis; `scale`: *positive*.
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

    /// Build from parts that are already known good.
    ///
    /// The invariant is closed under the operations below: composing or
    /// re-anchoring finite, positive-scale transforms yields another one,
    /// short of an overflow to infinity that finite inputs at this magnitude
    /// cannot reach. So the release checks [`Self::new`] owes a public caller
    /// handing in raw numbers are a debug contract here — and these run per
    /// transformed node per frame, where a release build must pay only the
    /// arithmetic.
    /// [`Self::new`]'s screen, one tier down and one strictness weaker,
    /// and the two differ by *rate* rather than by how much the invariant
    /// is worth. `new` is the door a caller builds a transform at, once.
    /// This is where the type's own arithmetic lands — `compose` runs in
    /// the cascade walk per transformed node per frame, and again in the
    /// composer per shape — so the check the door can afford is one this
    /// cannot. Overflow is how a derived one breaks: two finite scales
    /// multiply to `inf`.
    const fn from_parts(translation: Vec2, scale: f32) -> Self {
        debug_assert!(vec2::is_offset(translation), "{}", domain::OFFSET_RULE);
        debug_assert!(domain::is_positive(scale), "{}", domain::POSITIVE_RULE);
        Self { translation, scale }
    }

    /// Fold a pivot into a translation: `p ↦ (p - center) * s + center +
    /// translation` is `p * s + (center * (1 - s) + translation)`, and this
    /// is that parenthesised half. The one place the pivot algebra lives; the
    /// three constructors below differ in where the pivot comes from and in
    /// whether they validate the result.
    const fn pivoted_translation(translation: Vec2, center: Vec2, s: f32) -> Vec2 {
        Vec2::new(
            center.x * (1.0 - s) + translation.x,
            center.y * (1.0 - s) + translation.y,
        )
    }

    /// Move by `t`, at unit scale.
    pub const fn from_translation(t: Vec2) -> Self {
        Self::new(t, 1.0)
    }

    /// Scale by `s` about the origin. For a scale about a pivot, see
    /// [`Self::from_scale_about`].
    pub const fn from_scale(s: f32) -> Self {
        Self::new(Vec2::ZERO, s)
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
        Self::new(Self::pivoted_translation(Vec2::ZERO, center, s), s)
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
        Self::new(Self::pivoted_translation(translation, center, s), s)
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
    ///
    /// Re-anchors an already-valid `self`, so it builds
    /// `from_parts` rather than revalidating through
    /// [`Self::from_translate_scale_about`] — this is the cascade's
    /// per-transformed-node path.
    pub const fn anchored_at(self, origin: Vec2) -> Self {
        Self::from_parts(
            Self::pivoted_translation(self.translation, origin, self.scale),
            self.scale,
        )
    }

    /// Apply `self` after `other`: `result(p) == self.apply_point(other.apply_point(p))`.
    /// Matches matrix multiplication conventions — descend the tree by composing
    /// `parent_cumulative.compose(child_local)`.
    ///
    /// Both operands are already valid, and the invariant is closed under
    /// composition, so this is the bare 3×mul + 3×add in a release build —
    /// see `from_parts` for what that costs in debug.
    pub const fn compose(self, other: Self) -> Self {
        Self::from_parts(
            Vec2::new(
                other.translation.x * self.scale + self.translation.x,
                other.translation.y * self.scale + self.translation.y,
            ),
            self.scale * other.scale,
        )
    }

    /// Map a point through this transform. [`Self::inverse_vector`] is
    /// the reverse trip for a direction or an offset, which the
    /// translation does not apply to.
    pub const fn apply_point(self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x * self.scale + self.translation.x,
            p.y * self.scale + self.translation.y,
        )
    }

    /// Undo this transform for a direction or offset, where translation does
    /// not apply.
    pub const fn inverse_vector(self, v: Vec2) -> Vec2 {
        Vec2::new(v.x / self.scale, v.y / self.scale)
    }

    /// [`Self::apply_point`] for a whole rect — origin and extent both.
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

    /// A transform is the identity when its bits are, or when each part
    /// is within `EPS` of it. `-0.0` has other bits than `0.0`, so it
    /// takes the `EPS` fallback, as does the drift a lerp leaves behind.
    /// One pixel, or a 1.5 scale, is past the fallback.
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

    /// The type's own arithmetic held to the same contract as its door.
    ///
    /// Debug-only, and by *rate* rather than by worth: `from_parts` is
    /// where `compose` lands, which the cascade runs per transformed node
    /// per frame and the composer again per shape. Overflow is how a
    /// derived transform breaks — two finite scales multiply to `inf`.
    #[cfg(debug_assertions)]
    #[test]
    fn derived_transforms_reject_the_overflow_their_arithmetic_produces() {
        const TRANSLATION: &str = domain::OFFSET_RULE;
        const SCALE: &str = domain::POSITIVE_RULE;
        // Pivot arithmetic that overflows translation.
        panic_probe::assert_panics_with(TRANSLATION, || {
            TranslateScale::from_scale_about(Vec2::splat(f32::MAX), f32::MAX)
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
