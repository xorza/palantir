use crate::primitives::packed::half_simd::F16x4;
use half::f16;

/// The SWAR lane compare, checked exhaustively against the scalar form: all 65 536 f16 bit patterns in each of four lane positions at both thresholds. A carry escaping into a neighboring lane would show only for specific patterns.
#[test]
fn any_lane_above_matches_the_scalar_compare_for_every_pattern() {
    const F16_INFINITY: u16 = 0x7C00;

    for threshold in [F16_INFINITY, F16x4::EPS_BITS, 0, 0x7FFF] {
        for bits in 0..=u16::MAX {
            let scalar = (bits & 0x7FFF) > threshold;
            for lane in 0..4 {
                let mut lanes = [0u16; 4];
                lanes[lane] = bits;
                // Only `lane` is non-zero, so the answer must equal the scalar verdict (unless the threshold is 0).
                assert_eq!(
                    F16x4::from_bits(lanes).any_lane_above(threshold),
                    scalar,
                    "threshold={threshold:#06x} bits={bits:#06x} lane={lane}",
                );
            }
        }
    }

    // `has_nan` is that sweep at the infinity threshold; pin it against real f16 semantics.
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

    // A saturated neighbor must not leak a carry into the lane under test.
    for lane in 0..4 {
        let mut lanes = [0x7FFFu16; 4];
        lanes[lane] = 0;
        assert!(
            F16x4::from_bits(lanes).any_lane_above(F16_INFINITY),
            "saturated neighbors must still report above",
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

/// [`F16x4::scaled`] must be bit-identical to `from_lanes(lanes().map(* k))`, swept over every f16 pattern in a rotating lane arrangement against scale classes that could round differently (identity, zero, sign flip, halving, overflow, non-terminating fraction).
#[test]
fn scaled_is_bit_identical_to_the_composed_round_trip() {
    let composed = |bits: F16x4, k: f32| F16x4::from_lanes(bits.lanes().map(|v| v * k));
    for hi in 0..=u16::MAX {
        // Rotate the pattern across lanes so a lane-index mistake cannot hide.
        let bits = F16x4::from_bits([hi, hi ^ 0x3C00, hi.wrapping_add(0x1234), !hi]);
        for k in [1.0f32, 0.0, -3.0, 0.5, 1.0e4, 1.0 / 3.0] {
            assert_eq!(
                bits.scaled(k),
                composed(bits, k),
                "bits={:#06x?} k={k}",
                bits.0,
            );
        }
    }
}

/// Packing matches `half`'s scalar round-to-nearest-even bit for bit. Row one is exact in f16 so it also unpacks to itself; 1.1 in row two has no f16 form.
#[test]
fn packing_matches_scalar_quantization() {
    for (src, exact) in [
        ([0.0f32, 1.0, -2.5, 0.125], true),
        ([1.1, 1.2, 1.3, 1.4], false),
    ] {
        let packed = F16x4::from_lanes(src);
        let expected = src.map(|x| f16::from_f32(x).to_bits());
        assert_eq!(packed.0, expected, "{src:?}");
        assert_eq!(packed.lanes() == src, exact, "{src:?} round trip");
        // The `u64` word is the four lanes' bytes in lane order.
        assert_eq!(
            packed.as_u64(),
            u64::from_ne_bytes(bytemuck::cast(expected)),
            "{src:?}"
        );
    }
}

#[test]
fn to_f32_matches_scalar_reference_exhaustively() {
    // Every f16 bit pattern (including subnormals, ±inf, NaNs) decodes like `half`'s scalar path.
    for b in 0..=u16::MAX {
        let got = F16x4::from_bits([b; 4]).lanes().map(f32::to_bits);
        let want = f16::from_bits(b).to_f32().to_bits();
        assert_eq!(got, [want; 4], "bits = {b:#06x}");
    }
}

#[test]
fn from_f32_matches_scalar_reference_on_sweep() {
    // Quantization across magnitudes bracketing f16's range (subnormal, normal, overflow-to-inf, both signs), bit-exact against `half`.
    for i in 0..20_000u32 {
        let x = (i as f32 - 10_000.0) * 7.3;
        let tiny = (i as f32 - 10_000.0) * 1.0e-8;
        for v in [x, tiny] {
            let got = F16x4::from_lanes([v; 4]).0;
            let want = f16::from_f32(v).to_bits();
            assert_eq!(got[0], want, "v = {v}");
        }
    }
    let inf = F16x4::from_lanes([1.0e6, -1.0e6, f32::INFINITY, 0.0]).0;
    assert_eq!(inf[0], f16::INFINITY.to_bits());
    assert_eq!(inf[1], f16::NEG_INFINITY.to_bits());
    assert_eq!(inf[2], f16::INFINITY.to_bits());
    assert_eq!(inf[3], 0);
}
