use crate::primitives::approx::EPS;
use crate::primitives::half_simd::*;
use half::f16;

/// The SWAR lane compare, checked **exhaustively** against the
/// scalar form it replaces: every one of the 65 536 f16 bit
/// patterns, in each of the four lane positions, at both thresholds
/// the crate uses.
///
/// Exhaustive because the failure mode it guards is a carry
/// escaping one lane into its neighbour — which would show up for
/// specific bit patterns near the top of a lane's range and be
/// invisible to hand-picked cases. Cheap enough to be worth it: the
/// whole sweep is a few hundred thousand ALU ops.
#[test]
fn any_lane_above_matches_the_scalar_compare_for_every_pattern() {
    const F16_INFINITY: u16 = 0x7C00;
    const EPS_BITS: u16 = f16::from_f32_const(EPS).to_bits();

    for threshold in [F16_INFINITY, EPS_BITS, 0, 0x7FFF] {
        for bits in 0..=u16::MAX {
            let scalar = (bits & 0x7FFF) > threshold;
            for lane in 0..4 {
                let mut lanes = [0u16; 4];
                lanes[lane] = bits;
                // Lane `lane` is the only non-zero one, so the
                // answer must be exactly the scalar verdict for it
                // — unless the threshold is 0, where the zero lanes
                // are themselves not above it either.
                assert_eq!(
                    F16x4::from_bits(lanes).any_lane_above(threshold),
                    scalar,
                    "threshold={threshold:#06x} bits={bits:#06x} lane={lane}",
                );
            }
        }
    }

    // `has_nan` is that sweep at the infinity threshold, so pin it
    // against real f16 semantics rather than against itself.
    for bits in 0..=u16::MAX {
        let want = f16::from_bits(bits).is_nan();
        for lane in 0..4 {
            let mut lanes = [0u16; 4];
            lanes[lane] = bits;
            assert_eq!(
                F16x4::from_bits(lanes).has_nan(),
                want,
                "bits={bits:#06x} lane={lane}",
            );
        }
    }

    // A saturated neighbour must not leak a carry into the lane
    // under test — the specific thing SWAR could get wrong.
    for lane in 0..4 {
        let mut lanes = [0x7FFFu16; 4];
        lanes[lane] = 0;
        assert!(
            F16x4::from_bits(lanes).any_lane_above(F16_INFINITY),
            "saturated neighbours must still report above",
        );
        let mut lanes = [0u16; 4];
        lanes[lane] = 0x7FFF;
        assert!(
            F16x4::from_bits(lanes).any_lane_above(F16_INFINITY),
            "lane {lane}"
        );
    }
    assert!(
        !F16x4::from_bits([0x7BFF; 4]).any_lane_above(F16_INFINITY),
        "the largest finite f16 is not above infinity",
    );
    assert!(
        !F16x4::from_bits([F16_INFINITY; 4]).any_lane_above(F16_INFINITY),
        "infinity itself is not *above* infinity — that boundary is \
         what makes the NaN test exact",
    );
}

/// [`f16x4_scaled`] is a hand-written SIMD chain that replaced the
/// composed `from_lanes(lanes().map(* k))`, so the property that
/// matters is that it is **bit-identical** to what it replaced —
/// not merely close. Swept over every f16 bit pattern in a rotating
/// lane arrangement, against every scale class that could round
/// differently: identity, zero, sign flip, halving, a value that
/// overflows f16's range, and a non-terminating fraction.
#[test]
fn scaled_is_bit_identical_to_the_composed_round_trip() {
    let composed = |bits: [u16; 4], k: f32| f16x4_from_f32x4(f16x4_to_f32x4(bits).map(|v| v * k));
    for hi in 0..=u16::MAX {
        // Rotate the pattern across lanes so a lane-index mistake
        // cannot hide behind four identical lanes.
        let bits = [hi, hi ^ 0x3C00, hi.wrapping_add(0x1234), !hi];
        for k in [1.0f32, 0.0, -3.0, 0.5, 1.0e4, 1.0 / 3.0] {
            assert_eq!(
                f16x4_scaled(bits, k),
                composed(bits, k),
                "bits={bits:#06x?} k={k}",
            );
        }
    }
}

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

#[test]
fn to_f32_matches_scalar_reference_exhaustively() {
    // Every f16 bit pattern (including subnormals, ±inf, NaNs) must
    // decode exactly like `half`'s scalar path — this cross-checks
    // whichever SIMD/dispatch path the build selected.
    for b in 0..=u16::MAX {
        let got = f16x4_to_f32x4([b; 4]).map(f32::to_bits);
        let want = f16::from_bits(b).to_f32().to_bits();
        assert_eq!(got[0], want, "bits = {b:#06x}");
        assert_eq!(got, [got[0]; 4], "lane divergence at {b:#06x}");
    }
}

#[test]
fn from_f32_matches_scalar_reference_on_sweep() {
    // Quantization sweep across magnitudes bracketing f16's range:
    // subnormal (< 6.1e-5), normal, overflow-to-inf (> 65504), plus
    // sign coverage. Bit-exact against `half`'s scalar RTNE.
    for i in 0..20_000u32 {
        let x = (i as f32 - 10_000.0) * 7.3;
        let tiny = (i as f32 - 10_000.0) * 1.0e-8;
        for v in [x, tiny] {
            let got = f16x4_from_f32x4([v; 4]);
            let want = f16::from_f32(v).to_bits();
            assert_eq!(got[0], want, "v = {v}");
        }
    }
    let inf = f16x4_from_f32x4([1.0e6, -1.0e6, f32::INFINITY, 0.0]);
    assert_eq!(inf[0], f16::INFINITY.to_bits());
    assert_eq!(inf[1], f16::NEG_INFINITY.to_bits());
    assert_eq!(inf[2], f16::INFINITY.to_bits());
    assert_eq!(inf[3], 0);
}

/// The pre-F16C scalar fallbacks, called directly.
///
/// Gated exactly as they are, and it has to be direct: on a machine that
/// *has* F16C the runtime detect inside `f16x4_from_f32x4` takes the
/// intrinsic branch, so the sweeps above never reach these even in a
/// baseline build. Without this, the fallback's only coverage would be
/// running the suite on pre-2012 hardware.
///
/// Checked against the intrinsic rather than against `half` again: the
/// property that matters is that a machine without F16C encodes
/// *identically* to one with it, so a value doesn't change meaning with
/// the host CPU.
#[cfg(all(target_arch = "x86_64", not(target_feature = "f16c")))]
#[test]
fn scalar_fallbacks_match_the_intrinsic() {
    use crate::primitives::half_simd::{f16x4_from_f32x4_scalar, f16x4_to_f32x4_scalar};

    for sign in [1.0f32, -1.0] {
        for exp in -20i32..20 {
            let m = sign * 1.37 * 2.0f32.powi(exp);
            let lanes = [m, m * 0.5, m * 255.0, 0.0];
            assert_eq!(
                f16x4_from_f32x4_scalar(lanes),
                f16x4_from_f32x4(lanes),
                "encode {lanes:?}",
            );
        }
    }
    for bits in 0..=u16::MAX {
        let packed = [bits, bits ^ 0x3C00, !bits, 0];
        assert_eq!(
            f16x4_to_f32x4_scalar(packed).map(f32::to_bits),
            f16x4_to_f32x4(packed).map(f32::to_bits),
            "decode {packed:#06x?}",
        );
    }
}
