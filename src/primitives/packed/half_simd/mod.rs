//! Direct 4-lane f16 ↔ f32 pack/unpack, plus the [`F16x4`] newtype that `Spacing`, `Corners`, `RgbaF16`, `FillAxis` and `LoweredShadow` wrap.
//!
//! Bypasses `half`'s slice converters, which pay a runtime `is_x86_feature_detected!("f16c")` and an out-of-line call per use. Each architecture's conversions are a file of their own, picked here and nowhere else: `x86_64` runs the F16C intrinsics, `portable` takes `half`'s slice path.

#[cfg(not(target_arch = "x86_64"))]
mod portable;
#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(not(target_arch = "x86_64"))]
use crate::primitives::packed::half_simd::portable as arch;
#[cfg(target_arch = "x86_64")]
use crate::primitives::packed::half_simd::x86_64 as arch;

use crate::primitives::math::domain::EPS;
use std::hash;

/// Four f16 lanes packed in 8 B (`[u16; 4]`, align 2): the shared storage core behind `Corners`, `Spacing`, `FillAxis`, `RgbaF16` and `LoweredShadow`'s geometry. It owns the pack/unpack/hash/NaN idioms; lane meaning is the wrapper's business.
///
/// `Pod` with `repr(transparent)`, so wrappers keep the `[u16; 4]` GPU-wire layout. A wrapper forwards only what it has a caller for.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct F16x4([u16; 4]);

impl F16x4 {
    /// The crate's visual tolerance as f16 bits, and the mask and exponent the lane predicates classify against. Lives here because a lane pattern is not an f32 question.
    const EPS_BITS: u16 = half::f16::from_f32_const(EPS).to_bits();
    const ONE_MINUS_EPS_BITS: u16 = half::f16::from_f32_const(1.0 - EPS).to_bits();
    const ABS_MASK: u16 = 0x7FFF;
    const NAN_EXP: u16 = 0x7C00;

    pub(crate) const ZERO: Self = Self([0; 4]);

    pub(crate) const ONE: Self = Self([half::f16::ONE.to_bits(); 4]);

    pub(crate) const MAX_LANE: f32 = half::f16::MAX.to_f32_const();

    #[inline(always)]
    pub(crate) const fn lane_bits(self, lane: usize) -> u16 {
        self.0[lane]
    }

    /// Wrap four raw lane words. Lanes are f16 except a triangle quad's corner points, which are unorm16 (decoded by `fill_kind`).
    #[inline(always)]
    pub(crate) const fn from_bits(bits: [u16; 4]) -> Self {
        Self(bits)
    }

    #[inline]
    pub(crate) fn from_lanes(lanes: [f32; 4]) -> Self {
        Self(arch::from_f32x4(lanes))
    }

    /// True if any lane's magnitude exceeds the f16 bit pattern `bits` (sign ignored). `bits` must be `<= 0x7FFF`.
    ///
    /// SWAR: with the sign masked every lane is at most `0x7FFF`, so adding `0x7FFF - bits` can set a lane's top bit but never carries out of it, and all four compares run as one masked add. Packing by shift keeps it `const` and endian-independent.
    #[inline]
    pub(crate) const fn any_lane_above(self, bits: u16) -> bool {
        const ABS: u64 = 0x7FFF_7FFF_7FFF_7FFF;
        const SIGN: u64 = 0x8000_8000_8000_8000;
        debug_assert!(bits <= 0x7FFF, "threshold must be a magnitude pattern");
        let [a, b, c, d] = self.0;
        let packed = (a as u64) | ((b as u64) << 16) | ((c as u64) << 32) | ((d as u64) << 48);
        let bias = (0x7FFF - bits) as u64;
        let bias = bias | (bias << 16) | (bias << 32) | (bias << 48);
        ((packed & ABS) + bias) & SIGN != 0
    }

    #[inline]
    pub(crate) const fn any_lane_non_finite(self) -> bool {
        self.any_lane_above(Self::NAN_EXP - 1)
    }

    /// True if any lane is below zero. `-0.0` is not.
    #[inline]
    pub(crate) const fn any_lane_negative(self) -> bool {
        let mut i = 0;
        while i < 4 {
            let lane = self.0[i];
            if lane & !Self::ABS_MASK != 0 && lane & Self::ABS_MASK != 0 {
                return true;
            }
            i += 1;
        }
        false
    }

    /// True if any lane is NaN: [`Self::any_lane_above`] at infinity.
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        self.any_lane_above(Self::NAN_EXP)
    }

    /// True when every lane is within [`EPS`] of zero. NaN reports non-zero, so a fast path gated on this is never taken on an undefined lane.
    #[inline]
    pub(crate) const fn all_lanes_noop(self) -> bool {
        !self.any_lane_above(Self::EPS_BITS)
    }

    /// True when `lane` is at most [`EPS`], every negative value included, as
    /// [`domain::is_invisible`](crate::primitives::math::domain::is_invisible) reads an f32.
    /// Compares bit patterns directly, since positive f16 values are monotonic in their bits; a
    /// NaN of either sign reads as above it.
    #[inline]
    pub(crate) const fn lane_is_noop(self, lane: usize) -> bool {
        let bits = self.lane_bits(lane);
        let negative = bits & !Self::ABS_MASK != 0 && bits & Self::ABS_MASK <= Self::NAN_EXP;
        bits <= Self::EPS_BITS || negative
    }

    /// True when `lane` is within [`EPS`] of 1.0. The upper bound rejects NaN, and a negative lane's sign bit puts it above `NAN_EXP`, rejecting it too.
    #[inline]
    pub(crate) const fn lane_is_opaque(self, lane: usize) -> bool {
        let bits = self.lane_bits(lane);
        bits >= Self::ONE_MINUS_EPS_BITS && bits < Self::NAN_EXP
    }

    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        arch::to_f32x4(self.0)
    }

    /// Per-lane f32 multiply, re-quantized through f16.
    ///
    /// Fused because `from_lanes(lanes().map(*k))` bounces through two `[f32; 4]` arrays that LLVM does not fold back into one register chain.
    #[inline]
    pub(crate) fn scaled(self, k: f32) -> Self {
        Self(arch::scaled(self.0, k))
    }

    /// The 8 storage bytes as one `u64`, for a single hasher write.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        // Native bytes in lane order, as a `bytemuck` cast would read them, but `const`.
        let [a, b, c, d] = self.0;
        let [a, b, c, d] = [
            a.to_ne_bytes(),
            b.to_ne_bytes(),
            c.to_ne_bytes(),
            d.to_ne_bytes(),
        ];
        u64::from_ne_bytes([a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]])
    }
}

impl hash::Hash for F16x4 {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.as_u64());
    }
}

#[cfg(feature = "internals")]
pub(crate) mod bench;
#[cfg(test)]
mod tests;
