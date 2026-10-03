//! Scalar helpers the engines keep one definition of: the [`F32Px`] and
//! [`Vec2Ext`] methods on the scalars themselves, and a free conversion
//! whose exact form is a contract rather than a detail.

use glam::Vec2;

/// A 0..1 value as a byte: rounded half up, saturating outside the
/// range, and zero for NaN.
///
/// The saturation is the whole body. Rust's float→int `as` is saturating
/// by language guarantee, not by LLVM accident, so NaN already yields 0,
/// anything under the range already yields 0, and anything over it
/// already yields `u8::MAX`. Range checks would be three predicates paid
/// per channel per colour to reach what the final instruction reaches
/// anyway. Adding the half before the truncation is round-half-up, which
/// over a non-negative product is `round`.
///
/// A free `const fn` rather than an [`F32Px`] method: `RgbaF32::hexa` is
/// `const`, and a trait method cannot be called from one.
#[inline]
#[expect(
    clippy::cast_sign_loss,
    reason = "the saturating cast is the clamp: NaN and anything below the range land on zero"
)]
pub(crate) const fn unit_to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5) as u8
}

/// The `f32` operations the layout and paint engines keep one definition
/// of. The snap and quantize family replaces a libm call the hot paths
/// cannot afford, and says so at each; the gap count is the one every
/// stacking container spells.
///
/// Crate-private on purpose: nothing a widget does needs them, and the
/// grids they snap to are cache identities, not an API.
pub(crate) trait F32Px {
    /// This gap laid *between* `count` items: `count - 1` of them, and
    /// none at all for one item or none.
    ///
    /// One definition because every container that stacks children spells
    /// it — the two stacks, the wrap stack's lines, a grid's tracks and
    /// each span inside them — and each of them once for measure and
    /// again for arrange. The saturating step is the whole content: an
    /// empty container has no gaps, and `0 - 1` on a `usize` is not zero.
    fn gaps_between(self, count: usize) -> f32;

    /// Exact `f32::round` (round half away from zero) without the libm
    /// call: baseline x86-64 has no `roundss` (SSE4.1), so `.round()`
    /// compiles to an out-of-line `roundf` call in the per-quad snap
    /// and pixel-alignment paths. Integer-pipeline trick from Go 1.10's
    /// `math.Round`: add a half-ulp at the fraction position (the
    /// mantissa carry performs the round-up), then clear the fraction.
    /// Bit-identical to `f32::round` for every f32 bit pattern —
    /// including NaN payloads, ±inf, and `(-0.5, -0.0]` → `-0.0` —
    /// at ~3.5× the speed of the libm call.
    fn fast_round(self) -> f32;

    /// The whole pixel that covers `self` — `ceil` as the `u32` every
    /// caller of it wants, without the out-of-line `ceilf` baseline
    /// x86-64 makes of `f32::ceil` (no SSE4.1 `roundss`) — the same
    /// reason [`Self::fast_round`] exists, on the same per-quad scissor
    /// path.
    ///
    /// Truncate, then bump when the truncation lost something. Exact for
    /// every non-negative value: below `2^24` a `u32` round-trips through
    /// `f32`, and from `2^24` up every `f32` is already whole, so the
    /// truncation is the answer, saturating at `u32::MAX`.
    fn ceil_px(self) -> u32;

    /// `self` has no fractional part — equivalent to `x == x.round()`
    /// minus the libm call. NaN reports `false` like the equality it
    /// replaces; magnitudes ≥ 2^63 (unreachable for pixel coordinates)
    /// report `false`, which only forgoes a fast path.
    fn is_integral(&self) -> bool;

    /// Snap to the whole-pixel grid that cache identities key on, as an
    /// integer so the result can be compared and hashed exactly.
    ///
    /// One definition on purpose: a measure-cache `available_q` and a text
    /// run's wrap width both quantize through here, and were they to land
    /// on different grids a cached subtree could be blitted against a shape
    /// measured at another width. Non-finite (an unbounded axis) saturates
    /// rather than wrapping through the `as` cast.
    fn quantize_px(self) -> i32;

    /// [`Self::quantize_px`]'s grid, back in `f32`, for the extents that
    /// are *compared against* a cache key rather than hashed into one.
    ///
    /// Every discontinuous decision taken against an available extent
    /// owes this: a line break, a truncation, a fit test. The key its
    /// answer is cached under holds whole pixels, so a decision taken on
    /// the fraction can fall the other side of a boundary from the one
    /// the key stands for, and a warm frame then answers what a cold one
    /// would not. A continuous output — a flex shrink, a track share —
    /// has no boundary to fall the wrong side of, and reads the raw
    /// extent.
    ///
    /// A negative extent names no space and answers zero, which is what
    /// an over-constrained layout's callers want of a width driven below
    /// nothing.
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
            // |x| < 1: ±0, or ±1 once |x| ≥ 0.5 (e == BIAS - 1).
            bits &= SIGN_MASK;
            if e == BIAS - 1 {
                bits |= ONE;
            }
        } else if e < BIAS + SHIFT {
            // Fraction bits exist: the half-ulp add carries through the
            // mantissa (into the exponent at a .5 crossing — that IS the
            // round-up), the mask clears what's left of the fraction.
            let e = e - BIAS;
            bits += HALF >> e;
            bits &= !(FRAC_MASK >> e);
        }
        // e ≥ BIAS + SHIFT: already integral, or inf/NaN — unchanged.
        f32::from_bits(bits)
    }

    #[inline]
    #[expect(
        clippy::cast_sign_loss,
        reason = "the input is a non-negative pixel coordinate, debug-asserted before the cast"
    )]
    fn ceil_px(self) -> u32 {
        // Any magnitude: from 2^24 up every f32 is a whole number, so the
        // truncation below is already the ceiling, saturating at
        // `u32::MAX` past the range.
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
    /// Componentwise [`F32Px::fast_round`]. `Vec2::round` is two
    /// out-of-line `roundf` calls on baseline x86-64, which is what this
    /// exists to keep off the per-icon and per-quad snap paths.
    fn fast_round(self) -> Vec2;
}

impl Vec2Ext for Vec2 {
    #[inline]
    fn fast_round(self) -> Vec2 {
        Vec2::new(self.x.fast_round(), self.y.fast_round())
    }
}

/// Marker trait for primitive numeric types accepted by `From` impls on
/// `Sizing`, `Size`, `Corners`, `Spacing`, etc.
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
