/// A NaN edge is reportable, and reported per-lane — the four are one
/// `u64` and a check that only looked at the first would pass a NaN
/// bottom margin straight into layout.
///
/// Paired with `Corners`, which is the same eight bytes: the two
/// agreeing here is what says the screening `Corners` has had all
/// along now covers spacing too.
#[test]
fn a_nan_on_any_edge_is_screened_like_a_nan_corner() {
    use crate::primitives::corners::Corners;
    use crate::primitives::nan::NanCheck;

    assert!(!Spacing::all(4.0).has_nan(), "a whole spacing is finite");
    for lane in 0..4 {
        let mut lanes = [1.0, 2.0, 3.0, 4.0];
        lanes[lane] = f32::NAN;
        assert!(
            NanCheck::has_nan(&Spacing::from_array(lanes)),
            "lane {lane} went unseen",
        );
        assert!(
            NanCheck::has_nan(&Corners::from_array(lanes)),
            "lane {lane} went unseen on the sibling",
        );
    }
}

use crate::primitives::serde::test_support::{from_ron, ron_text};
use crate::primitives::spacing::*;

#[test]
fn lanes_round_trip_integer_values_exactly() {
    let s = Spacing::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(s.as_array(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(s.horizontal_sum(), 4.0);
    assert_eq!(s.vertical_sum(), 6.0);
}

/// Documents the f16 precision contract.
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
        assert_eq!(Spacing::all(value).as_array()[0], stored, "{value}");
    }
}

#[test]
fn as_array_and_from_array_round_trip() {
    let original = Spacing::new(1.0, 2.0, 3.0, 4.0);
    let arr = original.as_array();
    assert_eq!(arr, [1.0, 2.0, 3.0, 4.0]);
    let rebuilt = Spacing::from_array(arr);
    assert_eq!(rebuilt, original);
}

#[test]
fn xy_ctor_repeats_axes() {
    let s = Spacing::xy(3.0, 7.0);
    assert_eq!(s.as_array(), [3.0, 7.0, 3.0, 7.0]);
    assert_eq!(s.horizontal_sum(), 6.0);
    assert_eq!(s.vertical_sum(), 14.0);
}

/// Tuple `From` impls — easy place to swap component order during
/// a refactor. Pin both forms.
#[test]
fn from_tuple_preserves_component_order() {
    let xy: Spacing = (3, 7).into();
    assert_eq!(xy.as_array(), [3.0, 7.0, 3.0, 7.0]);
    let ltrb: Spacing = (1, 2, 3, 4).into();
    assert_eq!(ltrb.as_array(), [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn serialize_picks_compact_form_per_symmetry() {
    let cases: &[(&str, Spacing, &str)] = &[
        ("uniform_scalar", Spacing::all(4.0), "4.0"),
        ("axis_pair_two_array", Spacing::xy(4.0, 8.0), "[4.0,8.0]"),
        (
            "asymmetric_four_array",
            Spacing::new(1.0, 2.0, 3.0, 4.0),
            "[1.0,2.0,3.0,4.0]",
        ),
        (
            "diagonal_match_does_not_collapse",
            Spacing::new(1.0, 1.0, 2.0, 2.0),
            "[1.0,1.0,2.0,2.0]",
        ),
    ];
    for (label, s, want) in cases {
        assert_eq!(ron_text(s), *want, "case: {label}");
    }
}

#[test]
fn deserialize_accepts_scalar_array_and_integer_forms() {
    let cases: &[(&str, &str, Spacing)] = &[
        ("scalar", "4.0", Spacing::all(4.0)),
        ("integer_scalar", "4", Spacing::all(4.0)),
        ("two_element_array", "[4.0,8.0]", Spacing::xy(4.0, 8.0)),
        (
            "four_element_array",
            "[1.0,2.0,3.0,4.0]",
            Spacing::new(1.0, 2.0, 3.0, 4.0),
        ),
        ("one_element_array_uniform", "[4.0]", Spacing::all(4.0)),
    ];
    for (label, input, want) in cases {
        assert_eq!(from_ron::<Spacing>(input), *want, "case: {label}");
    }
}

#[test]
fn deserialize_struct_form() {
    let text = "(left: 1.0, top: 2.0, right: 3.0, bottom: 4.0)";
    assert_eq!(from_ron::<Spacing>(text), Spacing::new(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn serialize_then_parse_round_trips() {
    for s in [
        Spacing::all(4.0),
        Spacing::xy(4.0, 8.0),
        Spacing::new(1.0, 2.0, 3.0, 4.0),
    ] {
        let out = ron_text(&s);
        assert_eq!(
            from_ron::<Spacing>(&out),
            s,
            "round-trip failed for {s:?} -> {out}"
        );
    }
}

#[test]
fn add_op() {
    let a = Spacing::new(1.0, 2.0, 3.0, 4.0);
    let b = Spacing::new(10.0, 20.0, 30.0, 40.0);
    let c = a + b;
    assert_eq!(c.as_array(), [11.0, 22.0, 33.0, 44.0]);
}
