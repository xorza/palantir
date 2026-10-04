use crate::primitives::geometry::corners::*;
use crate::primitives::math::domain::EPS;
use crate::primitives::packed::serde::internals::{from_ron, ron_text};

#[test]
fn lanes_round_trip_integer_values_exactly() {
    let c = Corners::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(c.as_array(), [1.0, 2.0, 3.0, 4.0]);
}

/// Documents the f16 precision contract. A refactor that quietly
/// switched storage (e.g. to Q8.8 fixed-point) would trip these.
#[test]
fn f16_precision_contract() {
    // Integers are exact to 2048. Past it the f16 step is 2, then 4 from
    // 4096, and a tie rounds to the even mantissa: 2049 → 2048, 2051 →
    // 2052, 4097 and 4098 → 4096, 4099 → 4100.
    for (value, stored) in [
        (2048.0, 2048.0),
        (2049.0, 2048.0),
        (2051.0, 2052.0),
        (4096.0, 4096.0),
        (4097.0, 4096.0),
        (4098.0, 4096.0),
        (4099.0, 4100.0),
    ] {
        assert_eq!(Corners::all(value).as_array()[0], stored, "{value}");
    }
}

#[test]
fn as_array_and_from_array_round_trip() {
    let original = Corners::new(1.0, 2.0, 3.0, 4.0);
    let arr = original.as_array();
    assert_eq!(arr, [1.0, 2.0, 3.0, 4.0]);
    let rebuilt = Corners::from_array(arr);
    assert_eq!(rebuilt, original);
}

/// `f = min(side / sum of its two radii)`, hand-computed per row; every
/// radius scales by `f` when it is below 1.
#[test]
fn fit_to_scales_overlapping_radii_by_the_tightest_side() {
    let size = Size::new(100.0, 40.0);
    let rows = [
        // f = 40 / (9999 + 9999) on the short sides.
        ("all 9999", Corners::all(9999.0), [20.0; 4]),
        // f = 40 / 60.
        ("all 30", Corners::all(30.0), [20.0; 4]),
        // Sides: top 40 of 100, bottom 0, left 30 of 40, right 10: f ≥ 1.
        (
            "tl 30, tr 10",
            Corners::new(30.0, 10.0, 0.0, 0.0),
            [30.0, 10.0, 0.0, 0.0],
        ),
        // Left side: 40 / (40 + 40) = 0.5.
        (
            "tl 40, bl 40",
            Corners::new(40.0, 0.0, 0.0, 40.0),
            [20.0, 0.0, 0.0, 20.0],
        ),
        ("sharp", Corners::ZERO, [0.0; 4]),
    ];
    for (label, corners, want) in rows {
        assert_eq!(corners.fit_to(size, 1.0, 0.0).as_array(), want, "{label}");
    }
    // Scaled first, in f32: 9999 × 8 is past f16's 65504, and still fits
    // a 100×40 box at 8× (800×320) to 160.
    assert_eq!(
        Corners::all(9999.0)
            .fit_to(Size::new(800.0, 320.0), 8.0, 0.0)
            .as_array(),
        [160.0; 4],
    );
}

/// CSS `box-shadow` spread on a radius, in a box large enough that the
/// fit changes nothing: `(r 0, s 10)` stays sharp; `(r 10, s 10)` → 20;
/// `(r 4, s 10)` → `4 + 10·(1 + (0.4 − 1)³) = 4 + 10·0.784 = 11.84`;
/// `(r 10, s −6)` → 4; `(r 4, s −6)` → 0.
#[test]
fn fit_to_grows_radii_by_the_css_spread_rule() {
    let size = Size::new(1000.0, 1000.0);
    for (r, spread, want) in [
        (0.0, 10.0, 0.0),
        (10.0, 10.0, 20.0),
        (4.0, 10.0, 11.84),
        (10.0, -6.0, 4.0),
        (4.0, -6.0, 0.0),
    ] {
        let got = Corners::all(r).fit_to(size, 1.0, spread).as_array()[0];
        // f16 packing: 11.84 lands on the nearest step, 2^-7 apart there —
        // 1515.52 steps, so 1516 × 2^-7 = 11.84375.
        let packed = half::f16::from_f32(want).to_f32();
        assert_eq!(got, packed, "r {r}, spread {spread}");
    }
}

#[test]
fn scaled_by_multiplies_each_corner() {
    let c = Corners::new(2.0, 4.0, 6.0, 8.0).scaled_by(1.5);
    assert_eq!(c.as_array(), [3.0, 6.0, 9.0, 12.0]);
}

/// Pins the bit-trick path in `is_approx_zero`. ±0 lanes, sub-EPS,
/// at-EPS, above-EPS, and NaN must all classify correctly.
#[test]
fn approx_zero_handles_edge_lane_patterns() {
    assert!(Corners::ZERO.is_approx_zero(), "all-zero bytes");
    assert!(Corners::all(0.0).is_approx_zero(), "+0.0 lanes");
    assert!(
        Corners::all(-0.0).is_approx_zero(),
        "-0.0 lanes (sign bit set)"
    );
    assert!(Corners::all(EPS * 0.5).is_approx_zero(), "sub-EPS positive");
    assert!(
        !Corners::all(EPS * 10.0).is_approx_zero(),
        "10×EPS must NOT register as zero",
    );
    // One asymmetric lane above EPS — short-circuit must not
    // accept it just because the other three lanes are zero.
    assert!(
        !Corners::new(0.0, 0.0, 1.0, 0.0).is_approx_zero(),
        "single non-zero lane breaks zero contract",
    );
    // NaN bits land in the exponent region (≥ 0x7C00 absolute),
    // far above the EPS threshold — must classify as non-zero.
    assert!(
        !Corners::all(f32::NAN).is_approx_zero(),
        "NaN lanes are not zero"
    );
}

#[test]
fn tuples_map_to_lanes() {
    // The 2-tuple pairs by edge, like every other 2-value form here.
    assert_eq!(
        Corners::from((3.0, 7.0)).as_array(),
        [3.0, 3.0, 7.0, 7.0],
        "(top, bottom) → (t,t,b,b)",
    );
    assert_eq!(
        Corners::from((1, 2, 3, 4)).as_array(),
        [1.0, 2.0, 3.0, 4.0],
        "(tl, tr, br, bl) → lane order",
    );
}

/// The packed form the chrome hash writes: four f16 lanes, little-endian
/// `tl | tr | br | bl`. 1.0 is `0x3c00` and 2.0 is `0x4000` in f16, so
/// `(1, 2, 1, 2)` packs to `0x4000_3c00_4000_3c00`.
#[test]
fn as_u64_packs_the_four_lanes() {
    assert_eq!(Corners::from((1, 2, 1, 2)).as_u64(), 0x4000_3c00_4000_3c00);
    assert_eq!(Corners::ZERO.as_u64(), 0);
}

#[test]
fn convenience_ctors() {
    assert_eq!(Corners::top(4.0).as_array(), [4.0, 4.0, 0.0, 0.0]);
    assert_eq!(Corners::bottom(4.0).as_array(), [0.0, 0.0, 4.0, 4.0]);
    assert_eq!(Corners::left(4.0).as_array(), [4.0, 0.0, 0.0, 4.0]);
    assert_eq!(Corners::right(4.0).as_array(), [0.0, 4.0, 4.0, 0.0]);
    assert_eq!(
        Corners::top_bottom(2.0, 8.0).as_array(),
        [2.0, 2.0, 8.0, 8.0]
    );
    assert_eq!(Corners::diag_main(5.0).as_array(), [5.0, 0.0, 5.0, 0.0]);
    assert_eq!(Corners::diag_anti(5.0).as_array(), [0.0, 5.0, 0.0, 5.0]);
}

#[test]
fn serialize_picks_compact_form_per_symmetry() {
    let cases: &[(&str, Corners, &str)] = &[
        ("uniform_scalar", Corners::all(4.0), "4.0"),
        (
            "matched_pairs_two_array",
            Corners::new(4.0, 4.0, 8.0, 8.0),
            "[4.0,8.0]",
        ),
        (
            "asymmetric_four_array",
            Corners::new(1.0, 2.0, 3.0, 4.0),
            "[1.0,2.0,3.0,4.0]",
        ),
        (
            "near_matched_does_not_collapse",
            Corners::new(1.0, 2.0, 1.0, 2.0),
            "[1.0,2.0,1.0,2.0]",
        ),
    ];
    for (label, c, want) in cases {
        assert_eq!(ron_text(c), *want, "case: {label}");
    }
}

#[test]
fn deserialize_accepts_scalar_array_and_integer_forms() {
    let cases: &[(&str, &str, Corners)] = &[
        ("scalar", "4.0", Corners::all(4.0)),
        ("integer_scalar", "4", Corners::all(4.0)),
        (
            "two_element_array",
            "[4.0,8.0]",
            Corners::new(4.0, 4.0, 8.0, 8.0),
        ),
        (
            "four_element_array",
            "[1.0,2.0,3.0,4.0]",
            Corners::new(1.0, 2.0, 3.0, 4.0),
        ),
        ("one_element_array_uniform", "[4.0]", Corners::all(4.0)),
    ];
    for (label, input, want) in cases {
        assert_eq!(from_ron::<Corners>(input), *want, "case: {label}");
    }
}

/// A file radius is a finite, non-negative f16: 0 and 65504 are the
/// ends, and -1, one past 65504, infinity and NaN fail with the rule.
#[test]
fn deserialize_rejects_a_radius_f16_cannot_hold() {
    for (input, valid) in [
        ("0.0", true),
        ("65504.0", true),
        ("-1.0", false),
        ("65505.0", false),
        ("inf", false),
        ("NaN", false),
    ] {
        let parsed = ron::from_str::<Corners>(input);
        assert_eq!(parsed.is_ok(), valid, "{input}: {parsed:?}");
        if let Err(error) = parsed {
            assert!(
                error.to_string().contains(Corners::LANE_RULE),
                "{input}: {error}"
            );
        }
    }
}

#[test]
fn deserialize_struct_form() {
    let text = "(tl: 1.0, tr: 2.0, br: 3.0, bl: 4.0)";
    assert_eq!(from_ron::<Corners>(text), Corners::new(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn serialize_then_parse_round_trips() {
    for c in [
        Corners::all(4.0),
        Corners::new(4.0, 4.0, 8.0, 8.0),
        Corners::new(1.0, 2.0, 3.0, 4.0),
    ] {
        let s = ron_text(&c);
        assert_eq!(
            from_ron::<Corners>(&s),
            c,
            "round-trip failed for {c:?} -> {s}"
        );
    }
}
