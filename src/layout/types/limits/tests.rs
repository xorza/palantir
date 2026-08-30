use crate::layout::types::limits::{
    MAX_PACKED_GAP, assert_valid_bounds, valid_lower_bound, valid_packed_gap, valid_upper_bound,
};
use crate::primitives::size::Size;

#[test]
fn layout_limits_distinguish_finite_values_upper_infinity_and_f16_capacity() {
    for value in [0.0, -0.0, 1.0, MAX_PACKED_GAP] {
        assert!(valid_lower_bound(value), "lower bound {value}");
        assert!(valid_upper_bound(value), "upper bound {value}");
        assert!(valid_packed_gap(value), "packed gap {value}");
    }

    assert!(valid_upper_bound(f32::INFINITY));
    assert!(!valid_lower_bound(f32::INFINITY));
    assert!(!valid_packed_gap(f32::INFINITY));
    assert!(valid_lower_bound(MAX_PACKED_GAP + 1.0));
    assert!(!valid_packed_gap(MAX_PACKED_GAP + 1.0));

    for value in [-1.0, f32::NEG_INFINITY, f32::NAN] {
        assert!(!valid_lower_bound(value), "lower bound {value}");
        assert!(!valid_upper_bound(value), "upper bound {value}");
        assert!(!valid_packed_gap(value), "packed gap {value}");
    }
}

/// The pair screen runs in every build, so a node rejects the triple a
/// `Track` already rejects rather than carrying an inverted pair down to
/// `f32::clamp` in the arrange pass.
///
/// An infinite *maximum* is the unbounded axis and stays legal; every
/// other way the pair can go wrong is listed, because the point of one
/// screen is that each case is answered here.
#[test]
fn bounds_pairs_are_rejected_in_every_build() {
    assert_valid_bounds(Size::new(10.0, 10.0), Size::new(10.0, f32::INFINITY));
    assert_valid_bounds(Size::ZERO, Size::INF);

    let cases: &[(Size, Size, &str)] = &[
        (
            Size::new(20.0, 0.0),
            Size::new(10.0, 10.0),
            "inverted width",
        ),
        (
            Size::new(0.0, 20.0),
            Size::new(10.0, 10.0),
            "inverted height",
        ),
        (Size::new(f32::NAN, 0.0), Size::INF, "NaN minimum"),
        (Size::ZERO, Size::new(f32::NAN, 10.0), "NaN maximum"),
        (Size::new(-1.0, 0.0), Size::INF, "negative minimum"),
        (Size::ZERO, Size::new(-1.0, 10.0), "negative maximum"),
        (Size::INF, Size::INF, "infinite minimum"),
    ];
    for &(min_size, max_size, label) in cases {
        assert!(
            std::panic::catch_unwind(|| assert_valid_bounds(min_size, max_size)).is_err(),
            "case `{label}` must panic",
        );
    }
}
