//! Scalar helpers the crate keeps one definition of: the [`F32Ext`] /
//! [`Vec2Ext`] methods on the scalars themselves, and two free
//! conversions whose exact form is a contract rather than a detail.

use crate::primitives::approx;
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
/// A free `const fn` rather than an [`F32Ext`] method: `RgbaF32::hexa` is
/// `const`, and a trait method cannot be called from one.
#[inline]
pub(crate) const fn unit_to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5) as u8
}

/// The `f32` operations the crate keeps one definition of, as methods on
/// the scalar itself.
///
/// Two families, and the reason each is here differs. The snap and
/// quantize pair replaces a libm call the hot paths cannot afford, and
/// says so at each; [`Self::unit_fraction_or`] is here because one wrong
/// answer about a share reaches layout as a panic.
pub trait F32Ext {
    /// Where `self` sits along a track of `extent` that reserves `band`
    /// to a centred thing the pointer drags, as a 0..1 share.
    ///
    /// A slider's knob and a splitter's rule are the same placement
    /// problem: a fixed-width object whose *centre* follows the pointer,
    /// so half the band comes off each end before the division and the
    /// usable travel is `extent - band`. A track with no travel left has
    /// no share to report and yields zero.
    /// What that zero means is the caller's, and the two callers
    /// disagree.
    ///
    /// The result is unclamped — a pointer outside the track reports
    /// outside `0..1`, and each caller pins it with the bounds it
    /// enforces, which is what [`Self::unit_fraction_or`] is for.
    fn band_fraction(self, extent: f32, band: f32) -> f32;

    /// This gap laid *between* `count` items: `count - 1` of them, and
    /// none at all for one item or none.
    ///
    /// One definition because every container that stacks children spells
    /// it — the two stacks, the wrap stack's lines, a grid's tracks and
    /// each span inside them — and each of them once for measure and
    /// again for arrange. The saturating step is the whole content: an
    /// empty container has no gaps, and `0 - 1` on a `usize` is not zero.
    fn gaps_between(self, count: usize) -> f32;

    /// This value as a share of something — clamped into `0..=1`, or
    /// `fallback` where it names no share at all.
    ///
    /// The clamp alone is not the rule. `f32::clamp` answers NaN for NaN,
    /// and every consumer of a share turns it into a `Fill` weight, a
    /// track extent, or a seam position — each of which rejects one. An
    /// infinity clamps to an *end*, which states a share the caller never
    /// meant. Both non-finite cases are "no share", so both take the
    /// fallback.
    ///
    /// `fallback` is the caller's, because "no share" resolves
    /// differently: unknown progress is empty, an unknown split is
    /// centred. The screen is shared, the neutral is not.
    fn unit_fraction_or(self, fallback: f32) -> f32;

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
    /// Truncate, then bump when the truncation lost something. For a
    /// non-negative coordinate below `2^24`, where a `u32` still
    /// round-trips through `f32` exactly — every caller is a pixel
    /// coordinate, and the debug assert is the guard.
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

    /// A length read out of a theme, floored at `min`.
    ///
    /// One definition because every widget that sizes a node or a corner
    /// radius from its bundle owes the same guard: the scalar arrived
    /// from a hand-edited theme file or an app's own bundle, so a
    /// negative or NaN one is bad data rather than a logic error and
    /// cannot assert. Both cases land on `min`, since `f32::max` answers
    /// the other operand for NaN.
    ///
    /// `min` is the widget's, not the type's. A rule the theme sets to
    /// zero is a rule the app wanted invisible, while a grab bar or a
    /// spinner that thin cannot be grabbed or seen at all.
    fn themed_length(self, min: f32) -> f32;
}

impl F32Ext for f32 {
    #[inline]
    fn band_fraction(self, extent: f32, band: f32) -> f32 {
        approx::ratio(self - band * 0.5, extent - band)
    }

    #[inline]
    fn gaps_between(self, count: usize) -> f32 {
        self * count.saturating_sub(1) as f32
    }

    #[inline]
    fn unit_fraction_or(self, fallback: f32) -> f32 {
        debug_assert!(
            (0.0..=1.0).contains(&fallback),
            "a unit-fraction fallback must itself be a share, got {fallback}",
        );
        if self.is_finite() {
            self.clamp(0.0, 1.0)
        } else {
            fallback
        }
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
    fn ceil_px(self) -> u32 {
        debug_assert!(
            (0.0..(1u32 << 24) as f32).contains(&self),
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
    fn quantize_px(self) -> i32 {
        if self.is_finite() {
            self.fast_round() as i32
        } else {
            i32::MAX
        }
    }

    #[inline]
    fn themed_length(self, min: f32) -> f32 {
        debug_assert!(
            min >= 0.0,
            "a themed length's floor is itself a length, got {min}",
        );
        self.max(min)
    }
}

/// [`F32Ext`] applied per component, for the paint paths that snap a point.
pub(crate) trait Vec2Ext {
    /// Componentwise [`F32Ext::fast_round`]. `Vec2::round` is two
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

macro_rules! impl_num {
    ($($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { self as f32 }
            }
        )*
    };
}

impl_num!(f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

#[cfg(test)]
mod tests;
