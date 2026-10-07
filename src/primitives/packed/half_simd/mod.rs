//! Direct 4-lane f16 ↔ f32 pack/unpack, plus the [`F16x4`] newtype that `Spacing`, `Corners`, `RgbaF16`, `FillAxis` and `LoweredShadow` wrap.
//!
//! Bypasses `half`'s slice converters, which pay a runtime `is_x86_feature_detected!("f16c")` and an out-of-line call per use. On x86_64 the F16C intrinsics run under a `#[target_feature]` unsafe inner; with static F16C the wrapper is one instruction, otherwise it branches on the cached detection. Pre-F16C x86 uses `half`'s scalar kernel (`from_f32_const`, since the detection is already settled). Other targets use `half`'s slice path.

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
        Self(f16x4_from_f32x4(lanes))
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

    /// True when `lane` is within [`EPS`] of zero. Compares bit patterns directly, since positive f16 values are monotonic in their bits; NaN lands above the threshold and reads as non-zero.
    #[inline]
    pub(crate) const fn lane_is_noop(self, lane: usize) -> bool {
        self.lane_bits(lane) & Self::ABS_MASK <= Self::EPS_BITS
    }

    /// True when `lane` is within [`EPS`] of 1.0. The upper bound rejects NaN, and a negative lane's sign bit puts it above `NAN_EXP`, rejecting it too.
    #[inline]
    pub(crate) const fn lane_is_opaque(self, lane: usize) -> bool {
        let bits = self.lane_bits(lane);
        bits >= Self::ONE_MINUS_EPS_BITS && bits < Self::NAN_EXP
    }

    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        f16x4_to_f32x4(self.0)
    }

    /// Per-lane f32 multiply, re-quantized through f16.
    ///
    /// Fused because `from_lanes(lanes().map(*k))` bounces through two `[f32; 4]` arrays that LLVM does not fold back into one register chain.
    #[inline]
    pub(crate) fn scaled(self, k: f32) -> Self {
        Self(f16x4_scaled(self.0, k))
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

/// The scalar encode every x86 fallback shares. `from_f32_const`, not `from_f32`: the public converter re-runs F16C detection per lane, which callers have already settled.
#[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
#[inline]
fn f16x4_from_f32x4_scalar(src: [f32; 4]) -> [u16; 4] {
    src.map(|v| half::f16::from_f32_const(v).to_bits())
}

#[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
#[inline]
fn f16x4_to_f32x4_scalar(bits: [u16; 4]) -> [f32; 4] {
    bits.map(|b| half::f16::from_bits(b).to_f32_const())
}

#[inline]
pub(crate) fn f16x4_to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: the static `f16c` cfg guarantees `_mm_cvtph_ps`.
        unsafe { f16x4_to_f32x4_f16c(bits) }
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
    {
        // SAFETY: the runtime detection guarantees `_mm_cvtph_ps`.
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { f16x4_to_f32x4_f16c(bits) };
        }
        f16x4_to_f32x4_scalar(bits)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // `half`'s slice path: `fcvtl` on aarch64-fp16, scalar elsewhere.
        use half::slice::HalfFloatSliceExt as _;
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&bits);
        let mut out = [0.0f32; 4];
        arr.convert_to_f32_slice(&mut out);
        out
    }
}

#[inline]
pub(crate) fn f16x4_from_f32x4(src: [f32; 4]) -> [u16; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: see `f16x4_to_f32x4`.
        unsafe { f16x4_from_f32x4_f16c(src) }
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
    {
        // SAFETY: see the runtime branch in `f16x4_to_f32x4`.
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { f16x4_from_f32x4_f16c(src) };
        }
        f16x4_from_f32x4_scalar(src)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        use half::slice::HalfFloatSliceExt as _;
        let mut out = [half::f16::ZERO; 4];
        out.convert_from_f32_slice(&src);
        bytemuck::cast(out)
    }
}

/// Decode, scale and re-encode in one pass; see [`F16x4::scaled`].
#[inline]
pub(crate) fn f16x4_scaled(bits: [u16; 4], k: f32) -> [u16; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: see `f16x4_to_f32x4`.
        unsafe { f16x4_scaled_f16c(bits, k) }
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
    {
        // SAFETY: see the runtime branch in `f16x4_to_f32x4`. One detect covers both conversions, so the fallback goes straight to the scalar pair.
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { f16x4_scaled_f16c(bits, k) };
        }
        f16x4_from_f32x4_scalar(f16x4_to_f32x4_scalar(bits).map(|v| v * k))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        f16x4_from_f32x4(f16x4_to_f32x4(bits).map(|v| v * k))
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "f16c")]
unsafe fn f16x4_scaled_f16c(bits: [u16; 4], k: f32) -> [u16; 4] {
    use std::arch::x86_64::{
        _MM_FROUND_TO_NEAREST_INT, _mm_cvtph_ps, _mm_cvtps_ph, _mm_loadl_epi64, _mm_mul_ps,
        _mm_set1_ps, _mm_storel_epi64,
    };
    // SAFETY: same accesses as the `_f16c` converters.
    unsafe {
        let lanes = _mm_cvtph_ps(_mm_loadl_epi64(bits.as_ptr().cast()));
        let packed = _mm_cvtps_ph::<_MM_FROUND_TO_NEAREST_INT>(_mm_mul_ps(lanes, _mm_set1_ps(k)));
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr().cast(), packed);
        out
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "f16c")]
unsafe fn f16x4_to_f32x4_f16c(bits: [u16; 4]) -> [f32; 4] {
    use std::arch::x86_64::{_mm_cvtph_ps, _mm_loadl_epi64};
    // SAFETY: 8 B load; `#[target_feature]` enforces F16C.
    unsafe {
        let v = _mm_loadl_epi64(bits.as_ptr().cast());
        let f = _mm_cvtph_ps(v);
        core::mem::transmute(f)
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "f16c")]
unsafe fn f16x4_from_f32x4_f16c(src: [f32; 4]) -> [u16; 4] {
    use std::arch::x86_64::{
        _MM_FROUND_TO_NEAREST_INT, _mm_cvtps_ph, _mm_loadu_ps, _mm_storel_epi64,
    };
    // SAFETY: 16 B load, 8 B store; matching array layouts.
    unsafe {
        let v = _mm_loadu_ps(src.as_ptr());
        let h = _mm_cvtps_ph::<{ _MM_FROUND_TO_NEAREST_INT }>(v);
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr().cast(), h);
        out
    }
}

#[cfg(feature = "bench")]
pub(crate) mod bench;
#[cfg(test)]
mod tests;
