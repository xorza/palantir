//! A 2D extent in logical pixels: a magnitude, not a position.

use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::math::num::Num;
use glam::{BVec2, Vec2};
use serde::de;
use std::hash;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
/// A 2D extent in logical pixels. Negative components are meaningless, and
/// [`Self::INF`] is the "no upper bound" sentinel measure passes down.
#[must_use]
pub struct Size {
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl hash::Hash for Size {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

impl FloatHash for Size {
    #[inline]
    fn hash_eq<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            (u64::from(float_hash::eq_bits(self.w)) << 32) | u64::from(float_hash::eq_bits(self.h)),
        );
    }

    #[inline]
    fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            (u64::from(float_hash::canon_bits(self.w)) << 32)
                | u64::from(float_hash::canon_bits(self.h)),
        );
    }
}

impl Size {
    /// Zero on both axes.
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    /// Infinity on both axes: the size an unbounded parent hands a child.
    pub const INF: Self = Self {
        w: f32::INFINITY,
        h: f32::INFINITY,
    };

    /// A size from width and height, in logical pixels.
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// True if both axes are within `EPS` of zero; see [`Self::is_paint_empty`] for the looser predicate.
    pub const fn is_approx_zero(self) -> bool {
        domain::is_approx_zero(self.w) && domain::is_approx_zero(self.h)
    }

    /// True when either axis is at or below `EPS`, NaN included: the "paints no pixels" predicate.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        domain::is_invisible(self.w) || domain::is_invisible(self.h)
    }

    /// True if either axis is NaN.
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        self.w.is_nan() || self.h.is_nan()
    }

    /// Per-axis minimum.
    pub const fn min(self, other: Self) -> Self {
        Self {
            w: self.w.min(other.w),
            h: self.h.min(other.h),
        }
    }
    /// Per-axis maximum.
    pub const fn max(self, other: Self) -> Self {
        Self {
            w: self.w.max(other.w),
            h: self.h.max(other.h),
        }
    }

    /// What is left of this extent past `offset`, floored at zero per axis.
    #[inline]
    pub(crate) const fn room_past(self, offset: Vec2) -> Self {
        Self {
            w: (self.w - offset.x).max(0.0),
            h: (self.h - offset.y).max(0.0),
        }
    }

    /// Per-lane select: this size's lane where `mask` is set, else `other`'s.
    #[inline]
    pub(crate) const fn select(self, mask: BVec2, other: Self) -> Self {
        Self {
            w: if mask.x { self.w } else { other.w },
            h: if mask.y { self.h } else { other.h },
        }
    }

    /// Both axes by one factor.
    #[inline]
    pub const fn scaled_by(self, factor: f32) -> Self {
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

impl From<Size> for Vec2 {
    #[inline]
    fn from(size: Size) -> Self {
        Self::new(size.w, size.h)
    }
}

/// Wire format: a `{w, h}` table with optional fields; a non-finite axis serializes as absent.
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

/// An omitted axis reads as unbounded, not zero; the shared four-lane codec takes the opposite neutral.
impl<'de> ::serde::Deserialize<'de> for Size {
    fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, ::serde::Deserialize)]
        struct RawSize {
            w: Option<f32>,
            h: Option<f32>,
        }

        let raw = RawSize::deserialize(deserializer)?;
        let size = Size::new(
            raw.w.unwrap_or(f32::INFINITY),
            raw.h.unwrap_or(f32::INFINITY),
        );
        // A file is untrusted: NaN or a negative axis would reach a bound assert.
        if size.w >= 0.0 && size.h >= 0.0 {
            Ok(size)
        } else {
            Err(de::Error::custom(format_args!(
                "a size axis must not be negative or NaN, got {size:?}"
            )))
        }
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
    use crate::primitives::geometry::size::Size;
    use glam::Vec2;

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
        assert_eq!(real.min(nan), real);
        assert_eq!(real.max(nan), real);
    }
}
