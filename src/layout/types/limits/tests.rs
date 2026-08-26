use crate::layout::types::limits::{
    MAX_PACKED_GAP, valid_gap, valid_lower_bound, valid_packed_gap, valid_upper_bound,
};

#[test]
fn layout_limits_distinguish_finite_values_upper_infinity_and_f16_capacity() {
    for value in [0.0, -0.0, 1.0, MAX_PACKED_GAP] {
        assert!(valid_lower_bound(value), "lower bound {value}");
        assert!(valid_upper_bound(value), "upper bound {value}");
        assert!(valid_gap(value), "gap {value}");
        assert!(valid_packed_gap(value), "packed gap {value}");
    }

    assert!(valid_upper_bound(f32::INFINITY));
    assert!(!valid_lower_bound(f32::INFINITY));
    assert!(!valid_gap(f32::INFINITY));
    assert!(!valid_packed_gap(f32::INFINITY));
    assert!(valid_gap(MAX_PACKED_GAP + 1.0));
    assert!(!valid_packed_gap(MAX_PACKED_GAP + 1.0));

    for value in [-1.0, f32::NEG_INFINITY, f32::NAN] {
        assert!(!valid_lower_bound(value), "lower bound {value}");
        assert!(!valid_upper_bound(value), "upper bound {value}");
        assert!(!valid_gap(value), "gap {value}");
        assert!(!valid_packed_gap(value), "packed gap {value}");
    }
}
