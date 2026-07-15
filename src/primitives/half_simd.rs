//! Direct 4-lane f16 ↔ f32 pack/unpack, plus the [`F16x4`] newtype that
//! `Spacing`, `Corners`, `ColorF16`, and `FillAxis` wrap as their shared
//! `[u16; 4]` lane-storage core.
//!
//! Bypasses `half::slice::HalfFloatSliceExt::convert_{to,from}_f32_slice`,
//! which gates every call on a runtime `is_x86_feature_detected!("f16c")`
//! lookup + cross-crate (non-inlinable without LTO) call into an
//! out-of-line SIMD wrapper. Both costs were visible at the top of the
//! `frame` bench profile (~3.2% combined self-time + an absorbed ~3%
//! attributed to the callers; net ~6% on `frame/cached`).
//!
//! The x86_64 path here uses `_mm_cvtph_ps` / `_mm_cvtps_ph` directly
//! under a `#[target_feature(enable = "f16c")]` unsafe inner, called
//! from a safe wrapper. With `.cargo/config.toml`'s `target-cpu=x86-64-v3`
//! the feature is statically enabled and the wrapper compiles to a
//! single instruction. The non-x86 fallback walks the four lanes via
//! `half::f16::{from_bits,to_f32}` / `from_f32` — no slice dispatch.

/// Four f16 lanes packed in 8 B (`[u16; 4]`, align 2) — the shared
/// storage core behind `Corners`, `Spacing`, `FillAxis`, and
/// `ColorF16`. Each of those wraps an `F16x4` for type safety and adds
/// its own lane-naming + domain methods; `F16x4` owns only the
/// pack/unpack/hash idiom so the four types can't drift apart.
///
/// `Pod`/`Zeroable` with `repr(transparent)`, so a `repr(transparent)`
/// wrapper of `F16x4` keeps the exact `[u16; 4]` GPU-wire layout. Lane
/// *meaning* (order, units) is entirely the wrapper's business.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct F16x4(pub(crate) [u16; 4]);

impl F16x4 {
    /// All-zero lanes (`0.0` in f16). Also the `Default`.
    pub(crate) const ZERO: Self = Self([0; 4]);

    /// Pack four runtime f32 lanes — single SIMD instruction on
    /// F16C/fp16 targets, scalar fallback elsewhere.
    #[inline]
    pub(crate) fn from_lanes(lanes: [f32; 4]) -> Self {
        Self(f16x4_from_f32x4(lanes))
    }

    /// Unpack all four lanes to f32 at once via the batched slice path.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        f16x4_to_f32x4(self.0)
    }

    /// Per-lane f32 multiply, re-quantized through the f16 round-trip.
    #[inline]
    pub(crate) fn scaled(self, k: f32) -> Self {
        let [a, b, c, d] = self.lanes();
        Self::from_lanes([a * k, b * k, c * k, d * k])
    }

    /// The 8 storage bytes as one `u64` — lets wrappers hash with a
    /// single hasher write instead of four `write_u16`s.
    #[inline]
    pub(crate) fn as_u64(self) -> u64 {
        u64::from_ne_bytes(bytemuck::cast(self.0))
    }
}

impl std::hash::Hash for F16x4 {
    /// One `u64` write — wrappers `#[derive(Hash)]` and delegate here.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.as_u64());
    }
}

#[cfg(any(test, not(all(target_arch = "x86_64", target_feature = "f16c"))))]
use half::f16;
#[cfg(not(all(target_arch = "x86_64", target_feature = "f16c")))]
use half::slice::HalfFloatSliceExt;

/// Decode four packed f16 bit-patterns to f32 lanes.
#[inline]
pub(crate) fn f16x4_to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: the `target_feature = "f16c"` cfg above is the
        // compile-time guarantee `_mm_cvtph_ps` requires.
        unsafe { f16x4_to_f32x4_f16c(bits) }
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "f16c")))]
    {
        // Routes through `half`'s slice path: on aarch64-fp16 this is
        // `fcvtl`; on x86_64 without static F16C it's a runtime CPUID
        // dispatch (matching the pre-refactor behavior on v1/v2 builds).
        let arr: &[f16; 4] = bytemuck::cast_ref(&bits);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        out
    }
}

/// Encode four f32 lanes to packed f16 bit-patterns (round-to-nearest-even).
#[inline]
pub(crate) fn f16x4_from_f32x4(src: [f32; 4]) -> [u16; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: see `f16x4_to_f32x4`.
        unsafe { f16x4_from_f32x4_f16c(src) }
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "f16c")))]
    {
        let mut out = [f16::ZERO; 4];
        out.as_mut_slice().convert_from_f32_slice(&src);
        bytemuck::cast(out)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
#[inline]
#[target_feature(enable = "f16c")]
unsafe fn f16x4_to_f32x4_f16c(bits: [u16; 4]) -> [f32; 4] {
    use std::arch::x86_64::{_mm_cvtph_ps, _mm_loadl_epi64};
    // SAFETY: 4×u16 = 8 B fits in the low half of an __m128i; `_mm_loadl_epi64`
    // reads 8 B from the pointer, `_mm_cvtph_ps` converts the low 4 f16 lanes
    // to 4 f32 lanes. F16C feature presence enforced by `#[target_feature]`.
    unsafe {
        let v = _mm_loadl_epi64(bits.as_ptr() as *const _);
        let f = _mm_cvtph_ps(v);
        core::mem::transmute(f)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
#[inline]
#[target_feature(enable = "f16c")]
unsafe fn f16x4_from_f32x4_f16c(src: [f32; 4]) -> [u16; 4] {
    use std::arch::x86_64::{
        _MM_FROUND_TO_NEAREST_INT, _mm_cvtps_ph, _mm_loadu_ps, _mm_storel_epi64,
    };
    // SAFETY: `_mm_loadu_ps` reads 16 B from `src`'s storage (matches the
    // array layout). `_mm_cvtps_ph` packs to 4×f16 in the low 8 B of the
    // result. `_mm_storel_epi64` writes those 8 B to `out`'s 4×u16 = 8 B.
    unsafe {
        let v = _mm_loadu_ps(src.as_ptr());
        let h = _mm_cvtps_ph::<{ _MM_FROUND_TO_NEAREST_INT }>(v);
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr() as *mut _, h);
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::half_simd::*;

    #[test]
    fn round_trip_matches_half_slice() {
        // Hand-picked: zero, normal positive, normal negative, sub-integer.
        let src = [0.0f32, 1.0, -2.5, 0.125];
        let packed = f16x4_from_f32x4(src);
        // f16 represents all four values exactly (|x| < 2048, mantissa fits).
        let expected = [
            f16::from_f32(src[0]).to_bits(),
            f16::from_f32(src[1]).to_bits(),
            f16::from_f32(src[2]).to_bits(),
            f16::from_f32(src[3]).to_bits(),
        ];
        assert_eq!(packed, expected);
        let unpacked = f16x4_to_f32x4(packed);
        assert_eq!(unpacked, src);
    }

    #[test]
    fn lossy_values_match_scalar_quantization() {
        // 1.1 is not f16-representable; quantization must match the scalar
        // round-to-nearest-even path bit-for-bit.
        let src = [1.1f32, 1.2, 1.3, 1.4];
        let packed = f16x4_from_f32x4(src);
        let expected = [
            f16::from_f32(src[0]).to_bits(),
            f16::from_f32(src[1]).to_bits(),
            f16::from_f32(src[2]).to_bits(),
            f16::from_f32(src[3]).to_bits(),
        ];
        assert_eq!(packed, expected);
    }
}
