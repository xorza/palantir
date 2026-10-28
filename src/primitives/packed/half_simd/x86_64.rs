//! The F16C conversions, under a `#[target_feature]` unsafe inner. With static F16C each wrapper is one instruction; otherwise it branches on the cached detection, and a pre-F16C CPU takes `half`'s scalar kernel (`from_f32_const`, since the detection is already settled).

use std::arch::x86_64::{
    _MM_FROUND_TO_NEAREST_INT, _mm_cvtph_ps, _mm_cvtps_ph, _mm_loadl_epi64, _mm_loadu_ps,
    _mm_max_ps, _mm_min_ps, _mm_mul_ps, _mm_set1_ps, _mm_storel_epi64,
};

#[inline]
pub(super) fn to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    #[cfg(target_feature = "f16c")]
    {
        unsafe { to_f32x4_f16c(bits) }
    }
    #[cfg(not(target_feature = "f16c"))]
    {
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { to_f32x4_f16c(bits) };
        }
        to_f32x4_scalar(bits)
    }
}

#[inline]
pub(super) fn from_f32x4(src: [f32; 4]) -> [u16; 4] {
    #[cfg(target_feature = "f16c")]
    {
        unsafe { from_f32x4_f16c(src) }
    }
    #[cfg(not(target_feature = "f16c"))]
    {
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { from_f32x4_f16c(src) };
        }
        from_f32x4_scalar(src)
    }
}

/// Decode, scale, clamp to `±limit` and re-encode in one pass; see
/// [`F16x4::scaled`](crate::primitives::packed::half_simd::F16x4::scaled).
#[inline]
pub(super) fn scaled(bits: [u16; 4], k: f32, limit: f32) -> [u16; 4] {
    #[cfg(target_feature = "f16c")]
    {
        unsafe { scaled_f16c(bits, k, limit) }
    }
    #[cfg(not(target_feature = "f16c"))]
    {
        if std::arch::is_x86_feature_detected!("f16c") {
            return unsafe { scaled_f16c(bits, k, limit) };
        }
        // One detect covers both conversions, so the fallback goes straight to the scalar pair.
        from_f32x4_scalar(to_f32x4_scalar(bits).map(|v| (v * k).clamp(-limit, limit)))
    }
}

/// The scalar encode every pre-F16C path shares. `from_f32_const`, not `from_f32`: the public converter re-runs F16C detection per lane, which callers have already settled.
#[cfg(not(target_feature = "f16c"))]
#[inline]
fn from_f32x4_scalar(src: [f32; 4]) -> [u16; 4] {
    src.map(|v| half::f16::from_f32_const(v).to_bits())
}

#[cfg(not(target_feature = "f16c"))]
#[inline]
fn to_f32x4_scalar(bits: [u16; 4]) -> [f32; 4] {
    bits.map(|b| half::f16::from_bits(b).to_f32_const())
}

#[inline]
#[target_feature(enable = "f16c")]
unsafe fn scaled_f16c(bits: [u16; 4], k: f32, limit: f32) -> [u16; 4] {
    unsafe {
        let lanes = _mm_cvtph_ps(_mm_loadl_epi64(bits.as_ptr().cast()));
        let product = _mm_mul_ps(lanes, _mm_set1_ps(k));
        // Each takes its second operand when either is NaN, so a NaN lane passes through as
        // `f32::clamp` passes it.
        let clamped = _mm_min_ps(_mm_set1_ps(limit), _mm_max_ps(_mm_set1_ps(-limit), product));
        let packed = _mm_cvtps_ph::<_MM_FROUND_TO_NEAREST_INT>(clamped);
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr().cast(), packed);
        out
    }
}

#[inline]
#[target_feature(enable = "f16c")]
unsafe fn to_f32x4_f16c(bits: [u16; 4]) -> [f32; 4] {
    unsafe {
        let v = _mm_loadl_epi64(bits.as_ptr().cast());
        let f = _mm_cvtph_ps(v);
        core::mem::transmute(f)
    }
}

#[inline]
#[target_feature(enable = "f16c")]
unsafe fn from_f32x4_f16c(src: [f32; 4]) -> [u16; 4] {
    unsafe {
        let v = _mm_loadu_ps(src.as_ptr());
        let h = _mm_cvtps_ph::<{ _MM_FROUND_TO_NEAREST_INT }>(v);
        let mut out = [0u16; 4];
        _mm_storel_epi64(out.as_mut_ptr().cast(), h);
        out
    }
}

/// The pre-F16C scalar fallbacks, called directly: on an F16C machine the runtime detect in [`from_f32x4`] takes the intrinsic branch, so the conversion sweeps never reach them. Checked against the intrinsic: both must encode identically.
#[cfg(all(test, not(target_feature = "f16c")))]
mod tests {
    use crate::primitives::packed::half_simd::x86_64::{
        from_f32x4, from_f32x4_scalar, to_f32x4, to_f32x4_scalar,
    };

    #[test]
    fn scalar_fallbacks_match_the_intrinsic() {
        for sign in [1.0f32, -1.0] {
            for exp in -20i32..20 {
                let m = sign * 1.37 * 2.0f32.powi(exp);
                let lanes = [m, m * 0.5, m * 255.0, 0.0];
                assert_eq!(
                    from_f32x4_scalar(lanes),
                    from_f32x4(lanes),
                    "encode {lanes:?}",
                );
            }
        }
        for bits in 0..=u16::MAX {
            let packed = [bits, bits ^ 0x3C00, !bits, 0];
            assert_eq!(
                to_f32x4_scalar(packed).map(f32::to_bits),
                to_f32x4(packed).map(f32::to_bits),
                "decode {packed:#06x?}",
            );
        }
    }
}
