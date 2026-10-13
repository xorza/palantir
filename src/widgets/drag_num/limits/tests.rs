use crate::widgets::drag_num::limits::Limits;

/// A reversed pair is the same range and clamps identically.
#[test]
fn ordering_is_tolerated_and_clamping_matches() {
    let forward = Limits::of(0.0_f32, 10.0);
    let reversed = Limits::of(10.0_f32, 0.0);
    assert_eq!(forward, reversed);
    assert_eq!((forward.lo, forward.hi), (0.0, 10.0));
    for (value, want) in [
        (5.0, 5.0),
        (-1.0, 0.0),
        (11.0, 10.0),
        (0.0, 0.0),
        (10.0, 10.0),
    ] {
        assert_eq!(forward.clamp(value), want, "clamp({value})");
        assert_eq!(reversed.clamp(value), want, "reversed clamp({value})");
    }
}

/// A degenerate pair collapses to its point; an infinite one passes every finite value.
#[test]
fn degenerate_and_infinite_pairs_stay_usable() {
    assert_eq!(Limits::of(3.0_f64, 3.0).clamp(9.0), 3.0);
    let unbounded = Limits::of(f64::NEG_INFINITY, f64::INFINITY);
    assert_eq!(unbounded.clamp(-1.0e300), -1.0e300);
    assert_eq!(unbounded.clamp(1.0e300), 1.0e300);
}

/// A NaN bound reports the other end for both lanes, so the clamp pins to a real number.
#[test]
fn a_nan_bound_falls_back_to_the_other_end() {
    let limits = Limits::of(f32::NAN, 4.0);
    assert_eq!((limits.lo, limits.hi), (4.0, 4.0));
    assert_eq!(limits.clamp(100.0), 4.0);
}
