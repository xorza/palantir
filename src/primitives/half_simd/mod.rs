//! Direct 4-lane f16 ↔ f32 pack/unpack, plus the [`F16x4`] newtype that
//! `Spacing`, `Corners`, `ColorF16`, `FillAxis`, and `LoweredShadow`
//! wrap as their shared `[u16; 4]` lane-storage core.
//!
//! Bypasses `half::slice::HalfFloatSliceExt::convert_{to,from}_f32_slice`,
//! which gates every call on a runtime `is_x86_feature_detected!("f16c")`
//! lookup + cross-crate (non-inlinable without LTO) call into an
//! out-of-line SIMD wrapper. Both costs were visible at the top of the
//! `frame` bench profile (~3.2% combined self-time + an absorbed ~3%
//! attributed to the callers; net ~6% on `frame/cached`).
//!
//! The x86_64 path uses `_mm_cvtph_ps` / `_mm_cvtps_ph` directly under a
//! `#[target_feature(enable = "f16c")]` unsafe inner, called from a safe
//! wrapper. With static F16C (a `target-cpu=x86-64-v3` build) the wrapper
//! compiles to a single instruction. On a baseline x86-64 build the
//! wrapper branches on `is_x86_feature_detected!` (a cached-atomic load
//! and bit test, predicted after the first call) and direct-calls the
//! same local kernel — skipping `half`'s out-of-line slice scaffolding,
//! which profiled at ~1.4% self plus the dispatch absorbed by callers.
//! Pre-F16C x86 (pre-2012) walks the lanes through `half`'s scalar
//! *kernel* — `f16::from_f32_const`, not `from_f32`, because the public
//! converter re-runs its own F16C detection per lane and that question is
//! already settled by the time the fallback is reached. The non-x86
//! fallback goes through `half`'s slice path (`fcvtl` on aarch64-fp16),
//! where the dispatch is the point.

/// Four f16 lanes packed in 8 B (`[u16; 4]`, align 2) — the shared
/// storage core behind `Corners`, `Spacing`, `FillAxis`, `ColorF16`,
/// and `LoweredShadow`'s geometry. Each wraps an `F16x4` for type
/// safety and adds its own lane-naming + domain methods; `F16x4` owns
/// the pack/unpack/hash/NaN idioms so they can't drift apart.
///
/// `Pod`/`Zeroable` with `repr(transparent)`, so a `repr(transparent)`
/// wrapper of `F16x4` keeps the exact `[u16; 4]` GPU-wire layout. Lane
/// *meaning* (order, units) is entirely the wrapper's business.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct F16x4([u16; 4]);

impl F16x4 {
    /// All-zero lanes (`0.0` in f16). Also the `Default`.
    pub(crate) const ZERO: Self = Self([0; 4]);

    /// One lane's f16 bit pattern — for predicates that test a single
    /// lane (`ColorF16`'s alpha) without unpacking all four to f32.
    #[inline(always)]
    pub(crate) const fn lane_bits(self, lane: usize) -> u16 {
        self.0[lane]
    }

    /// Pack four runtime f32 lanes — single SIMD instruction on
    /// F16C/fp16 targets, scalar fallback elsewhere.
    #[inline]
    pub(crate) fn from_lanes(lanes: [f32; 4]) -> Self {
        Self(f16x4_from_f32x4(lanes))
    }

    /// True if any lane's **magnitude** exceeds the f16 bit pattern
    /// `bits` (sign ignored). `bits` must be `<= 0x7FFF`.
    ///
    /// SWAR rather than four scalar compares: masking the sign bit caps
    /// every lane at `0x7FFF`, so adding `0x7FFF - bits` can set a
    /// lane's top bit but can never carry *out* of it — which is what
    /// lets all four comparisons run as one masked add over the packed
    /// `u64`. Measured ~5× the scalar form.
    ///
    /// Both f16 lane predicates in the crate are this one test:
    /// [`Self::has_nan`] and `Corners::approx_zero` (which passes the
    /// `EPS` pattern and negates).
    ///
    /// Packing by shift instead of a cast keeps this `const` and
    /// endian-independent; LLVM folds it back to a single 64-bit load.
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

    /// True if any lane is NaN.
    ///
    /// `0x7C00` is f16 infinity and NaN is the only thing whose
    /// magnitude sorts above it, so the NaN test *is*
    /// [`Self::any_lane_above`] at that threshold — one masked add for
    /// all four lanes, no per-lane branch.
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        const F16_INFINITY: u16 = 0x7C00;
        self.any_lane_above(F16_INFINITY)
    }

    /// Unpack all four lanes to f32 at once via the batched slice path.
    #[inline]
    pub(crate) fn lanes(self) -> [f32; 4] {
        f16x4_to_f32x4(self.0)
    }

    /// Per-lane f32 multiply, re-quantized through the f16 round-trip.
    ///
    /// Fused rather than `from_lanes(lanes().map(*k))`: that spelling
    /// bounces the values through two `[f32; 4]` arrays and LLVM does not
    /// weld the halves back into one register chain. Bit-identical
    /// output, and `bench::scaled` holds the margin at **1.3x**.
    ///
    /// It was 2.3x when written, against a runtime-dispatched build where
    /// the composed form also paid the feature check twice. A static-F16C
    /// build (this workspace sets `+f16c`) deletes that half of the win
    /// and leaves only the array round-trip — still worth fusing on a
    /// per-quad path, but no longer the margin the original note claimed.
    #[inline]
    pub(crate) fn scaled(self, k: f32) -> Self {
        Self(f16x4_scaled(self.0, k))
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

/// The scalar encode every x86 fallback below shares.
///
/// `from_f32_const`, **not** `from_f32`: `half`'s public converter runs
/// its own `is_x86_feature_detected!("f16c")` per lane, and every caller
/// of this has just answered that question — no. Going through it would
/// re-ask four times and route each lane through an out-of-line
/// dispatching call, on the one configuration that can least afford it.
/// `from_f32_const` is the fallback kernel directly.
#[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
#[inline]
fn f16x4_from_f32x4_scalar(src: [f32; 4]) -> [u16; 4] {
    src.map(|v| half::f16::from_f32_const(v).to_bits())
}

/// [`f16x4_from_f32x4_scalar`]'s decode direction, same reasoning.
#[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
#[inline]
fn f16x4_to_f32x4_scalar(bits: [u16; 4]) -> [f32; 4] {
    bits.map(|b| half::f16::from_bits(b).to_f32_const())
}

/// Decode four packed f16 bit-patterns to f32 lanes.
#[inline]
pub(crate) fn f16x4_to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: the `target_feature = "f16c"` cfg above is the
        // compile-time guarantee `_mm_cvtph_ps` requires.
        unsafe { f16x4_to_f32x4_f16c(bits) }
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
    {
        // SAFETY: the branch is the runtime guarantee `_mm_cvtph_ps`
        // requires; `is_x86_feature_detected!` is a cached-atomic load.
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { f16x4_to_f32x4_f16c(bits) };
        }
        f16x4_to_f32x4_scalar(bits)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // `half`'s slice path: `fcvtl` on aarch64-fp16, scalar elsewhere.
        let arr: &[half::f16; 4] = bytemuck::cast_ref(&bits);
        let mut out = [0.0f32; 4];
        half::slice::HalfFloatSliceExt::convert_to_f32_slice(arr.as_slice(), &mut out);
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
        let mut out = [half::f16::ZERO; 4];
        half::slice::HalfFloatSliceExt::convert_from_f32_slice(out.as_mut_slice(), &src);
        bytemuck::cast(out)
    }
}

/// Decode, scale, and re-encode in one pass — see [`F16x4::scaled`] for
/// why this is fused rather than composed from the two converters.
#[inline]
pub(crate) fn f16x4_scaled(bits: [u16; 4], k: f32) -> [u16; 4] {
    #[cfg(all(target_arch = "x86_64", target_feature = "f16c"))]
    {
        // SAFETY: see `f16x4_to_f32x4`.
        unsafe { f16x4_scaled_f16c(bits, k) }
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
    {
        // SAFETY: see the runtime branch in `f16x4_to_f32x4`. One
        // detect here rather than one per converter — which is also why
        // the fallback goes straight to the scalar pair instead of back
        // through `f16x4_from_f32x4(f16x4_to_f32x4(..))`, where each
        // converter would re-run the detect this branch already settled.
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
    // SAFETY: the loads/stores are the same 8 B and 16 B accesses
    // `f16x4_to_f32x4_f16c` / `f16x4_from_f32x4_f16c` make; the multiply
    // stays in the register between them. F16C presence enforced by
    // `#[target_feature]`.
    unsafe {
        let lanes = _mm_cvtph_ps(_mm_loadl_epi64(bits.as_ptr() as *const _));
        let packed = _mm_cvtps_ph::<_MM_FROUND_TO_NEAREST_INT>(_mm_mul_ps(lanes, _mm_set1_ps(k)));
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr() as *mut _, packed);
        out
    }
}

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "x86_64")]
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

/// Raw-lane construction, for tests and benches only.
///
/// Production code has no business with the storage form — wrappers go in
/// through [`F16x4::from_lanes`] and come out through [`F16x4::lanes`] or
/// [`F16x4::lane_bits`] — so gating these is what makes the private field
/// mean anything. A child module, because the field is private to this
/// one and children can see it.
///
/// Gated on `bench` rather than the usual `internals`: no integration
/// test reaches the storage form, so the wider gate would leave both
/// items dead in an `internals`-only build.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use crate::primitives::half_simd::F16x4;

    impl F16x4 {
        /// Wrap four already-encoded f16 lanes — a raw test pattern, or a
        /// value read back off the GPU wire.
        #[inline(always)]
        pub(crate) const fn from_bits(bits: [u16; 4]) -> Self {
            Self(bits)
        }
    }
}

#[cfg(feature = "bench")]
pub(crate) mod bench;
#[cfg(test)]
mod tests;
