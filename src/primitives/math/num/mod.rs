//! Scalar helpers the layout and paint engines share one definition of.

use glam::Vec2;

/// A 0..1 value as a byte: rounded half up, saturating, zero for NaN.
///
/// Rust's saturating float-to-int `as` is the clamp; range checks would
/// only repeat what the cast does.
#[inline]
#[expect(
    clippy::cast_sign_loss,
    reason = "the saturating cast is the clamp: NaN and anything below the range land on zero"
)]
pub(crate) const fn unit_to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5) as u8
}

/// `f32` operations the layout and paint engines share.
///
/// Crate-private: the snap grids are cache identities, not API.
pub(crate) trait F32Px {
    /// This gap laid *between* `count` items: `count - 1` of them, none for
    /// one item or none.
    fn gaps_between(self, count: usize) -> f32;

    /// Exact `f32::round` (half away from zero) without the libm call:
    /// baseline x86-64 has no `roundss`, so `.round()` is an out-of-line
    /// `roundf`. Bit-identical to `f32::round` for every bit pattern.
    fn fast_round(self) -> f32;

    /// `ceil` as a `u32`, without the out-of-line `ceilf` (same reason as
    /// [`Self::fast_round`]).
    ///
    /// Exact for non-negative values, saturating at `u32::MAX`.
    fn ceil_px(self) -> u32;

    /// `self` has no fractional part, like `x == x.round()` minus the libm
    /// call. NaN and magnitudes beyond `i64` report `false`.
    fn is_integral(&self) -> bool;

    /// Snap to the whole-pixel grid that cache keys use, as an integer so it
    /// compares and hashes exactly.
    ///
    /// One definition so the measure cache and text wrap widths share a grid.
    /// Non-finite saturates to `i32::MAX`.
    fn quantize_px(self) -> i32;

    /// [`Self::quantize_px`]'s grid in `f32`, for extents *compared against*
    /// a cache key rather than hashed into one.
    ///
    /// Discontinuous decisions (line break, truncation, fit test) owe this:
    /// a decision on the raw fraction can land on the other side of the
    /// boundary the key stands for. Continuous outputs read the raw extent.
    ///
    /// A negative extent answers zero.
    fn canonical_px(self) -> f32;
}

impl F32Px for f32 {
    #[inline]
    fn gaps_between(self, count: usize) -> f32 {
        self * count.saturating_sub(1) as f32
    }

    #[inline]
    fn fast_round(self) -> f32 {
        const SHIFT: u32 = 23;
        const BIAS: u32 = 127;
        const SIGN_MASK: u32 = 0x8000_0000;
        const FRAC_MASK: u32 = (1 << SHIFT) - 1;
        const HALF: u32 = 1 << (SHIFT - 1);
        const ONE: u32 = BIAS << SHIFT;
        let mut bits = self.to_bits();
        let e = (bits >> SHIFT) & 0xff;
        if e < BIAS {
            bits &= SIGN_MASK;
            if e == BIAS - 1 {
                bits |= ONE;
            }
        } else if e < BIAS + SHIFT {
            // The half-ulp add carries through the mantissa (into the exponent at
            // a .5 crossing, which is the round-up); the mask clears the fraction.
            let e = e - BIAS;
            bits += HALF >> e;
            bits &= !(FRAC_MASK >> e);
        }
        f32::from_bits(bits)
    }

    #[inline]
    #[expect(
        clippy::cast_sign_loss,
        reason = "the input is a non-negative pixel coordinate, debug-asserted before the cast"
    )]
    fn ceil_px(self) -> u32 {
        debug_assert!(
            self >= 0.0,
            "ceil_px is for a non-negative pixel coordinate, got {self}",
        );
        let truncated = self as u32;
        truncated.saturating_add(u32::from((truncated as f32) < self))
    }

    #[inline]
    fn is_integral(&self) -> bool {
        *self == (*self as i64 as f32)
    }

    #[inline]
    fn canonical_px(self) -> f32 {
        self.max(0.0).quantize_px() as f32
    }

    #[inline]
    fn quantize_px(self) -> i32 {
        if self.is_finite() {
            self.fast_round() as i32
        } else {
            i32::MAX
        }
    }
}

/// [`F32Px`] applied per component, for the paint paths that snap a point.
pub(crate) trait Vec2Ext {
    /// Componentwise [`F32Px::fast_round`], avoiding two `roundf` calls.
    fn fast_round(self) -> Vec2;
}

impl Vec2Ext for Vec2 {
    #[inline]
    fn fast_round(self) -> Vec2 {
        Vec2::new(self.x.fast_round(), self.y.fast_round())
    }
}

/// Primitive numeric types accepted by the `From` impls on `Sizing`,
/// `Size`, `Corners`, `Spacing`.
pub(crate) trait Num: Copy {
    fn as_f32(self) -> f32;
}

impl Num for f32 {
    fn as_f32(self) -> f32 {
        self
    }
}

macro_rules! impl_num {
    (lossless: $($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { f32::from(self) }
            }
        )*
    };
    (rounding: $($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { self as f32 }
            }
        )*
    };
}

impl_num!(lossless: i8, i16, u8, u16);
impl_num!(rounding: f64, i32, i64, isize, u32, u64, usize);

#[cfg(test)]
mod tests;
