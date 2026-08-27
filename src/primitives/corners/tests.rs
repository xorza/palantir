use crate::primitives::approx::EPS;
use crate::primitives::corners::*;

fn ser(v: Corners) -> String {
    ron::ser::to_string(&v).expect("serialize")
}

fn de(text: &str) -> Corners {
    ron::from_str(text).expect("parse")
}

#[test]
fn struct_is_eight_bytes() {
    assert_eq!(std::mem::size_of::<Corners>(), 8);
    // align 2 (not 8) so embedding inside `Quad` doesn't bump
    // Quad's alignment above 4 and introduce trailing pad bytes
    // that break the `Pod` no-padding contract.
}

#[test]
fn lanes_round_trip_integer_values_exactly() {
    let c = Corners::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(c.as_array(), [1.0, 2.0, 3.0, 4.0]);
}

/// Documents the f16 precision contract: lossless for integer
/// radii ≤ 2048, ~0.25 px quantization at 4096. A refactor that
/// quietly switched storage (e.g. to Q8.8 fixed-point) would
/// trip these.
#[test]
fn f16_precision_contract() {
    assert_eq!(Corners::all(2048.0).as_array()[0], 2048.0);
    let big = Corners::all(4096.0).as_array()[0];
    assert!(
        (big - 4096.0).abs() <= 0.25,
        "expected ≤0.25 px error at 4096, got {} -> {big}",
        (big - 4096.0).abs(),
    );
}

#[test]
fn as_array_and_from_array_round_trip() {
    let original = Corners::new(1.0, 2.0, 3.0, 4.0);
    let arr = original.as_array();
    assert_eq!(arr, [1.0, 2.0, 3.0, 4.0]);
    let rebuilt = Corners::from_array(arr);
    assert_eq!(rebuilt, original);
}

#[test]
fn scaled_by_multiplies_each_corner() {
    let c = Corners::new(2.0, 4.0, 6.0, 8.0).scaled_by(1.5);
    assert_eq!(c.as_array(), [3.0, 6.0, 9.0, 12.0]);
}

/// Pins the bit-trick path in `approx_zero`. ±0 lanes, sub-EPS,
/// at-EPS, above-EPS, and NaN must all classify correctly.
#[test]
fn approx_zero_handles_edge_lane_patterns() {
    assert!(Corners::ZERO.approx_zero(), "all-zero bytes");
    assert!(Corners::all(0.0).approx_zero(), "+0.0 lanes");
    assert!(
        Corners::all(-0.0).approx_zero(),
        "-0.0 lanes (sign bit set)"
    );
    assert!(Corners::all(EPS * 0.5).approx_zero(), "sub-EPS positive",);
    assert!(
        !Corners::all(EPS * 10.0).approx_zero(),
        "10×EPS must NOT register as zero",
    );
    // One asymmetric lane above EPS — short-circuit must not
    // accept it just because the other three lanes are zero.
    assert!(
        !Corners::new(0.0, 0.0, 1.0, 0.0).approx_zero(),
        "single non-zero lane breaks zero contract",
    );
    // NaN bits land in the exponent region (≥ 0x7C00 absolute),
    // far above the EPS threshold — must classify as non-zero.
    assert!(
        !Corners::all(f32::NAN).approx_zero(),
        "NaN lanes are not zero"
    );
}

#[test]
fn from_vec2_and_size_map_to_pairs() {
    use crate::primitives::size::Size;
    use glam::Vec2;
    assert_eq!(
        Corners::from(Vec2::new(3.0, 7.0)).as_array(),
        [3.0, 3.0, 7.0, 7.0],
        "Vec2 → (x,x,y,y)",
    );
    assert_eq!(
        Corners::from(Size::new(3.0, 7.0)).as_array(),
        [3.0, 3.0, 7.0, 7.0],
        "Size → (w,w,h,h)",
    );
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
        assert_eq!(ser(*c), *want, "case: {label}");
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
        assert_eq!(de(input), *want, "case: {label}");
    }
}

#[test]
fn deserialize_struct_form() {
    let text = "(tl: 1.0, tr: 2.0, br: 3.0, bl: 4.0)";
    assert_eq!(de(text), Corners::new(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn serialize_then_parse_round_trips() {
    for c in [
        Corners::all(4.0),
        Corners::new(4.0, 4.0, 8.0, 8.0),
        Corners::new(1.0, 2.0, 3.0, 4.0),
    ] {
        let s = ser(c);
        assert_eq!(de(&s), c, "round-trip failed for {c:?} -> {s}");
    }
}
