//! The Armv8.0 Advanced SIMD conversions, `fcvtl` and `fcvtn`, inline. Every AArch64 core has
//! them, so nothing is detected: `half`'s slice path detects FEAT_FP16, which only its half
//! arithmetic needs, and calls out of line per use, through these same two instructions.

use std::arch::aarch64::{float32x4_t, uint16x4_t, vdupq_n_f32, vmaxq_f32, vminq_f32, vmulq_n_f32};
use std::arch::asm;
use std::mem::transmute;

#[inline]
pub(super) fn to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    unsafe { transmute::<float32x4_t, [f32; 4]>(widen(transmute::<[u16; 4], uint16x4_t>(bits))) }
}

#[inline]
pub(super) fn from_f32x4(src: [f32; 4]) -> [u16; 4] {
    unsafe { transmute::<uint16x4_t, [u16; 4]>(narrow(transmute::<[f32; 4], float32x4_t>(src))) }
}

/// Decode, scale, clamp to `±limit` and re-encode in one pass; see
/// [`F16x4::scaled`](crate::primitives::packed::half_simd::F16x4::scaled).
#[inline]
pub(super) fn scaled(bits: [u16; 4], k: f32, limit: f32) -> [u16; 4] {
    unsafe {
        let product = vmulq_n_f32(widen(transmute::<[u16; 4], uint16x4_t>(bits)), k);
        // `fmax` and `fmin` return NaN when either operand is, so a NaN lane passes through as
        // `f32::clamp` passes it.
        let clamped = vminq_f32(vdupq_n_f32(limit), vmaxq_f32(vdupq_n_f32(-limit), product));
        transmute::<uint16x4_t, [u16; 4]>(narrow(clamped))
    }
}

#[inline]
fn widen(half: uint16x4_t) -> float32x4_t {
    let wide: float32x4_t;
    unsafe {
        asm!(
            "fcvtl {wide:v}.4s, {half:v}.4h",
            wide = lateout(vreg) wide,
            half = in(vreg) half,
            options(pure, nomem, nostack, preserves_flags),
        );
    }
    wide
}

/// Rounds to nearest, ties to even, under the default FPCR every Rust program runs with.
#[inline]
fn narrow(wide: float32x4_t) -> uint16x4_t {
    let half: uint16x4_t;
    unsafe {
        asm!(
            "fcvtn {half:v}.4h, {wide:v}.4s",
            half = lateout(vreg) half,
            wide = in(vreg) wide,
            options(pure, nomem, nostack, preserves_flags),
        );
    }
    half
}

/// The inline conversions against `half`'s scalar kernel, which a core without FEAT_FP16 takes
/// through the slice path: both must encode and decode identically.
#[cfg(test)]
mod tests {
    use crate::primitives::packed::half_simd::aarch64::{from_f32x4, scaled, to_f32x4};

    #[test]
    fn inline_conversions_match_the_scalar_kernel() {
        let encode = |src: [f32; 4]| src.map(|v| half::f16::from_f32_const(v).to_bits());
        for sign in [1.0f32, -1.0] {
            for exp in -30i32..20 {
                let m = sign * 1.37 * 2.0f32.powi(exp);
                let lanes = [m, m * 0.5, m * 255.0, 0.0];
                assert_eq!(from_f32x4(lanes), encode(lanes), "encode {lanes:?}");
            }
        }
        let edges = [
            f32::NAN,
            -f32::NAN,
            f32::from_bits(0x7FC0_1234),
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            65504.0,
            65520.0,
            f32::MIN_POSITIVE,
            5.960_464_5e-8,
            2.980_232_2e-8,
            1.0 + f32::EPSILON,
        ];
        let (quads, []) = edges.as_chunks::<4>() else {
            unreachable!("the edges fill whole quads")
        };
        for &lanes in quads {
            assert_eq!(from_f32x4(lanes), encode(lanes), "encode {lanes:?}");
        }
        for bits in 0..=u16::MAX {
            let packed = [bits, bits ^ 0x3C00, !bits, 0];
            assert_eq!(
                to_f32x4(packed).map(f32::to_bits),
                packed.map(|b| half::f16::from_bits(b).to_f32_const().to_bits()),
                "decode {packed:#06x?}",
            );
            let reference =
                encode(packed.map(|b| {
                    (half::f16::from_bits(b).to_f32_const() * 3.5).clamp(-65504.0, 65504.0)
                }));
            assert_eq!(
                scaled(packed, 3.5, 65504.0),
                reference,
                "scale {packed:#06x?}"
            );
        }
    }
}
