use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::*;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::serde::internals::{from_ron, ron_text};

/// A NaN edge is reported per lane: the four share one `u64`, so checking only the first would pass a NaN bottom corner.
#[test]
fn a_nan_on_any_corner_is_screened() {
    assert!(!Corners::all(4.0).has_nan(), "a whole corner set is finite");
    for lane in 0..4 {
        let mut lanes = [1.0, 2.0, 3.0, 4.0];
        lanes[lane] = f32::NAN;
        assert!(
            NanCheck::has_nan(&Corners::new(lanes[0], lanes[1], lanes[2], lanes[3])),
            "lane {lane} went unseen",
        );
    }
}

/// Every constructor refuses an edge that does not pack to a finite f16, on any lane: NaN, an
/// infinity, and 65520, the first magnitude f16 rounds to infinity. 65519 rounds down to 65504
/// and is taken.
#[test]
fn an_edge_past_f16_is_refused_where_it_is_built() {
    for bad in [f32::NAN, f32::INFINITY, 65520.0, -65520.0] {
        for lane in 0..4 {
            let mut lanes = [1.0, 2.0, 3.0, 4.0];
            lanes[lane] = bad;
            panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::from_array(lanes));
        }
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::all(bad));
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::xy(1.0, bad));
        panic_probe::assert_panics_with(Spacing::LANE_RULE, || Spacing::new(1.0, 2.0, 3.0, bad));
    }
    assert_eq!(
        Spacing::new(65519.0, -65519.0, 0.0, 1.0).as_array(),
        [65504.0, -65504.0, 0.0, 1.0]
    );
}

/// A derived spacing saturates where an authored one is refused: ±70000 and ±infinity land on
/// ±65504, and a NaN still panics.
#[test]
fn a_derived_spacing_saturates_at_the_f16_range() {
    assert_eq!(
        Spacing::saturating([70000.0, -70000.0, f32::INFINITY, 1.0]).as_array(),
        [65504.0, -65504.0, 65504.0, 1.0]
    );
    panic_probe::assert_panics_with(Spacing::LANE_RULE, || {
        Spacing::saturating([0.0, f32::NAN, 0.0, 0.0])
    });
}

#[test]
fn lanes_round_trip_integer_values_exactly() {
    let s = Spacing::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(s.as_array(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(s.sums(), Size::new(4.0, 6.0));
}

/// Documents the f16 precision contract.
#[test]
fn f16_precision_contract() {
    // Integers are exact to 2048; past it the f16 step is 2, then 4 from 4096, ties to even: 2049 → 2048, 2051 → 2052, 4097/4098 → 4096, 4099 → 4100.
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
    assert_eq!(s.sums(), Size::new(6.0, 14.0));
}

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

/// A file lane is any finite f32 an f16 lane can hold, either sign:
/// ±65504 is the largest, and one past it packs to infinity.
#[test]
fn deserialize_rejects_a_lane_past_f16() {
    for (input, valid) in [
        ("65504.0", true),
        ("-65504.0", true),
        ("65505.0", false),
        ("-65505.0", false),
        ("inf", false),
        ("NaN", false),
    ] {
        let parsed = ron::from_str::<Spacing>(input);
        assert_eq!(parsed.is_ok(), valid, "{input}: {parsed:?}");
        if let Err(error) = parsed {
            assert!(
                error.to_string().contains(Spacing::LANE_RULE),
                "{input}: {error}"
            );
        }
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
