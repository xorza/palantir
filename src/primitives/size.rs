//! A 2D extent in logical pixels — a magnitude rather than a position,
//! which is why it is not a `Vec2`.

use crate::primitives::nan::NanCheck;
use crate::primitives::{
    approx::{self, FloatHash},
    num::Num,
};
use glam::{BVec2, Vec2};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
/// A 2D extent in logical pixels. Distinct from a `Vec2` because it is a
/// *magnitude*, not a position: negative components are meaningless, and
/// [`Self::INF`] is the "no upper bound" sentinel measure passes down.
///
/// Hashing is approximate (`1e-4` tolerance) so a sub-pixel float wobble
/// doesn't invalidate the measure cache.
pub struct Size {
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl std::hash::Hash for Size {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// Both axes in one `write_u64` rather than two component calls: one hasher
/// round per size, matching [`Vec2`]'s packing.
impl FloatHash for Size {
    #[inline]
    fn hash_eq<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(((approx::eq_bits(self.w) as u64) << 32) | approx::eq_bits(self.h) as u64);
    }

    #[inline]
    fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            ((approx::canon_bits(self.w) as u64) << 32) | approx::canon_bits(self.h) as u64,
        );
    }
}

impl Size {
    /// Zero on both axes.
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    /// Positive infinity on both axes — the "unconstrained" available size
    /// an unbounded parent hands a child during measure.
    pub const INF: Self = Self {
        w: f32::INFINITY,
        h: f32::INFINITY,
    };

    /// A size from width and height, in logical pixels.
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// True if both axes are within `EPS` of zero — i.e. this size
    /// is approximately `Size::ZERO`. Strict (both-axis) semantic to
    /// match the crate's scalar `approx_zero` predicate.
    /// For "paints no pixels" use [`Self::is_paint_empty`] —
    /// different (looser) predicate.
    pub const fn approx_zero(self) -> bool {
        approx::approx_zero(self.w) && approx::approx_zero(self.h)
    }

    /// True when either axis is at or below `EPS` (including NaN /
    /// negative from degenerate construction). The shared "paints no
    /// pixels" predicate — call from any gate that wants to drop
    /// zero-extent geometry before emit / cache work runs.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        approx::noop_f32(self.w) || approx::noop_f32(self.h)
    }

    /// True if either axis is NaN. `const`, so the const predicates that
    /// need the sweep can call it; the [`NanCheck`] impl below delegates
    /// here rather than keeping a second copy of the field walk.
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        self.w.is_nan() || self.h.is_nan()
    }

    /// Per-axis minimum — clamping a desired size down to what's available.
    pub const fn min(self, other: Self) -> Self {
        Self {
            w: self.w.min(other.w),
            h: self.h.min(other.h),
        }
    }
    /// Per-axis maximum — applying an intrinsic-minimum floor.
    pub const fn max(self, other: Self) -> Self {
        Self {
            w: self.w.max(other.w),
            h: self.h.max(other.h),
        }
    }

    /// What is left of this extent past `offset`, per axis, floored at
    /// zero — the room a container that places a child at `offset` has
    /// left to give it.
    ///
    /// [`Rect::deflated_by`](crate::primitives::rect::Rect::deflated_by)'s
    /// leading half, for the callers that hold an extent rather than a
    /// rect. The whole extent from an offset origin overflows by exactly
    /// that offset, which is the bug this exists to make hard to write.
    #[inline]
    pub(crate) fn room_past(self, offset: Vec2) -> Self {
        Self {
            w: (self.w - offset.x).max(0.0),
            h: (self.h - offset.y).max(0.0),
        }
    }

    /// Per-lane select: this size's lane where `mask` is set, `other`'s
    /// where it is not.
    ///
    /// The shape four layout drivers spelled as a pair of `if`s over `.w`
    /// and `.h` — a Hug axis measuring against `INFINITY`, a panned axis
    /// contributing nothing, a canvas axis taking the room past a child.
    /// One body, so the two lanes cannot drift apart.
    #[inline]
    pub(crate) fn select(self, mask: BVec2, other: Self) -> Self {
        Self {
            w: if mask.x { self.w } else { other.w },
            h: if mask.y { self.h } else { other.h },
        }
    }

    /// Both axes by one factor — a content extent at a scroll's zoom, a
    /// margin at the same. Named because the per-axis spelling puts `w`
    /// and `h` a keystroke apart, and a swap between them compiles.
    #[inline]
    pub const fn scaled(self, factor: f32) -> Self {
        Self {
            w: self.w * factor,
            h: self.h * factor,
        }
    }
}

impl<T: Num> From<T> for Size {
    fn from(v: T) -> Self {
        let v = v.as_f32();
        Self { w: v, h: v }
    }
}

impl<W: Num, H: Num> From<(W, H)> for Size {
    fn from((w, h): (W, H)) -> Self {
        Self {
            w: w.as_f32(),
            h: h.as_f32(),
        }
    }
}

/// The extent as a vector, for the arithmetic that mixes it with a
/// position: an offset across it, a share of it, a centre inside it.
impl From<Size> for Vec2 {
    #[inline]
    fn from(size: Size) -> Self {
        Self::new(size.w, size.h)
    }
}

/// Wire format: a `{w, h}` table whose fields are optional, because
/// [`Size::INF`] — the "no upper bound" sentinel — has no finite
/// spelling. A non-finite axis serializes as absent and an absent axis
/// deserializes back to infinity.
impl ::serde::Serialize for Size {
    fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use ::serde::ser::SerializeStruct;
        let finite = |value: f32| value.is_finite().then_some(value);
        let mut state = serializer.serialize_struct("Size", 2)?;
        state.serialize_field("w", &finite(self.w))?;
        state.serialize_field("h", &finite(self.h))?;
        state.end()
    }
}

/// An omitted axis reads as **unbounded**, not zero: this type spells a
/// `max_size` bound as often as an extent, and infinity is what "no
/// bound on this axis" means. `Serialize` above is the same rule read
/// backwards — a non-finite lane is written as absent. The shared
/// four-lane codec in `primitives::serde` takes the opposite neutral
/// for the opposite reason, which is why `Size` writes its own.
impl<'de> ::serde::Deserialize<'de> for Size {
    fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, ::serde::Deserialize)]
        struct RawSize {
            w: Option<f32>,
            h: Option<f32>,
        }

        let raw = RawSize::deserialize(deserializer)?;
        Ok(Size::new(
            raw.w.unwrap_or(f32::INFINITY),
            raw.h.unwrap_or(f32::INFINITY),
        ))
    }
}

impl NanCheck for Size {
    #[inline]
    fn has_nan(&self) -> bool {
        Size::has_nan(*self)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::size::Size;
    use glam::Vec2;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_value(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn min_and_max_are_per_axis() {
        let a = Size::new(1.0, 8.0);
        let b = Size::new(4.0, 2.0);
        assert_eq!(a.min(b), Size::new(1.0, 2.0));
        assert_eq!(a.max(b), Size::new(4.0, 8.0));
        assert_eq!(Vec2::from(a), Vec2::new(1.0, 8.0));
    }

    #[test]
    fn min_and_max_ignore_nan_operand() {
        let nan = Size::new(f32::NAN, f32::NAN);
        let real = Size::new(3.0, 5.0);
        // `f32::min`/`max` ignore NaN when the other operand is a real
        // number — matches every other f32-pair reduction in the crate
        // (e.g. `Rect::union`/`intersect`).
        assert_eq!(real.min(nan), real);
        assert_eq!(real.max(nan), real);
    }

    #[test]
    fn equal_signed_zero_sizes_have_equal_hashes() {
        let positive = Size::new(0.0, 0.0);
        let negative = Size::new(-0.0, -0.0);

        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));
    }
}
