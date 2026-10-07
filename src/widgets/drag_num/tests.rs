//! Rounding, clamping, and the string round-trip both numeric variants take.

use crate::widgets::drag_num::{DragNum, Num, round_to_decimals};

const INF: f64 = f64::INFINITY;

#[test]
fn round_to_decimals_snaps_and_formats_short() {
    // A long value snaps to its 3-decimal display and prints without a tail.
    let r = round_to_decimals(1.984_573_845_634_985_2, 3);
    assert_eq!(r, 1.985);
    assert_eq!(format!("{r:?}"), "1.985");
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 2), 1.98);
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 0), 2.0);
    assert_eq!(format!("{:?}", round_to_decimals(0.1 + 0.2, 1)), "0.3");
    assert_eq!(round_to_decimals(12.3456, 2), 12.35);
    assert_eq!(round_to_decimals(-1.6789, 1), -1.7);
}

/// Past 2^53 an `f64` has no fractional digits and the rounding shift overflows (`1e308` at 2 decimals): those magnitudes pass through unchanged.
#[test]
fn round_to_decimals_leaves_an_unshiftable_magnitude_alone() {
    for v in [1e308, -1e308, f64::MAX, 1e17, -1e17] {
        for decimals in [0, 2, 15] {
            assert_eq!(round_to_decimals(v, decimals), v, "{v} at {decimals}");
        }
    }
    // The shift is skipped only where there is nothing to round: 2^53 is whole-only, a half below it still rounds up.
    let whole_only = 9_007_199_254_740_992.0;
    assert_eq!(round_to_decimals(whole_only, 0), whole_only);
    assert_eq!(round_to_decimals(0.5, 0), 1.0);
    assert!(round_to_decimals(f64::NAN, 2).is_nan());
}

#[test]
fn commit_drag_snaps_rounds_clamps_and_reports_change() {
    // Float: snaps to `decimals`, unbounded is a no-op clamp; the same anchor and offset land on the same value twice.
    let mut f = 0.0;
    let zero = Num::F64(0.0);
    assert!(DragNum::from(&mut f).commit_drag(zero, 1.984_573_845_634_985_2, 3, -INF, INF));
    assert_eq!(f, 1.985);
    assert!(!DragNum::from(&mut f).commit_drag(zero, 1.984_573_845_634_985_2, 3, -INF, INF));
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(Num::F64(1.5), 0.25, 2, -INF, INF));
    assert_eq!(f, 1.75);
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_value(50.0, 2, 0.0, 10.0));
    assert_eq!(f, 10.0);
    // A tiny negative wiggle at a 0.0 bound is stored as +0.0 (bit-exact) with no change report.
    let mut f = 0.0;
    assert!(!DragNum::from(&mut f).commit_value(-0.004, 2, 0.0, 1.0));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits(), "-0.0 normalized to +0.0");
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_value(7.6, 3, -INF, INF));
    assert_eq!(i, 8);
    assert!(!DragNum::from(&mut i).commit_value(7.6, 3, -INF, INF));
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_value(500.0, 0, 0.0, 100.0));
    assert_eq!(i, 100);
    // Int: a fractional bound closes in to the integers inside it (0.5..=10 holds no 0).
    let mut i = 5;
    assert!(DragNum::from(&mut i).commit_value(-3.0, 0, 0.5, 10.0));
    assert_eq!(i, 1);
    let mut i = -5;
    assert!(DragNum::from(&mut i).commit_value(3.0, 0, -10.0, -0.5));
    assert_eq!(i, -1);
    // A range holding no integer clamps between the two either side.
    let mut i = 5;
    assert!(DragNum::from(&mut i).commit_value(5.0, 0, 0.2, 0.8));
    assert_eq!(i, 1);
}

/// A float drag at a magnitude with no room for `decimals` keeps the value it reached: the snap is skipped, not overflowed.
#[test]
fn a_float_scrub_never_stores_an_infinite_value() {
    let mut f = 1e308_f64;
    assert!(!DragNum::from(&mut f).commit_drag(Num::F64(1e308), 0.0, 2, -INF, INF));
    assert_eq!(f, 1e308);
    // Moving further stays finite and the sum stands exactly.
    assert!(DragNum::from(&mut f).commit_drag(Num::F64(1e308), 1e307, 2, -INF, INF));
    assert_eq!(f, 1e308 + 1e307);
}

/// An integer scrub moves the exact anchor, so the whole `i64` domain survives; widening to `f64` first would round values past 2^53.
#[test]
fn an_integer_scrub_moves_the_exact_anchor() {
    let mut i = 9_007_199_254_740_993_i64;
    let anchor = DragNum::from(&mut i).read();
    assert_eq!(anchor, Num::I64(9_007_199_254_740_993));
    assert!(!DragNum::from(&mut i).commit_drag(anchor, 0.0, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_993);
    assert!(!DragNum::from(&mut i).commit_drag(anchor, 0.4, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_993);
    assert!(DragNum::from(&mut i).commit_drag(anchor, 2.4, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_995);
    // Travel the domain cannot hold saturates rather than wrapping.
    assert!(DragNum::from(&mut i).commit_drag(anchor, INF, 2, -INF, INF));
    assert_eq!(i, i64::MAX);
    let mut edge = i64::MIN;
    assert!(!DragNum::from(&mut edge).commit_drag(Num::I64(i64::MIN), -3.0, 2, -INF, INF));
    assert_eq!(edge, i64::MIN);
}

#[test]
fn drag_num_read_keeps_each_targets_precision() {
    let mut f = 2.5_f64;
    assert_eq!(DragNum::from(&mut f).read(), Num::F64(2.5));
    let mut i = 5_i64;
    assert_eq!(DragNum::from(&mut i).read(), Num::I64(5));
    // Widening is for visual quantities only and loses the low bit of an `i64` past 2^53.
    let mut i = 9_007_199_254_740_993_i64;
    assert_eq!(
        DragNum::from(&mut i).read(),
        Num::I64(9_007_199_254_740_993)
    );
    assert_eq!(
        DragNum::from(&mut i).read().widen(),
        9_007_199_254_740_992.0
    );
}

#[test]
fn drag_num_edit_string_and_parse_round_trip() {
    // Float keeps a trailing `.0` so it re-reads as a float; a fractional value survives verbatim.
    let mut f = 3.0_f64;
    assert_eq!(DragNum::from(&mut f).edit_string(), "3.0");
    let mut f = 2.5_f64;
    let s = DragNum::from(&mut f).edit_string();
    assert!(!DragNum::from(&mut f).parse_from(&s, -INF, INF));
    assert_eq!(f, 2.5);

    let mut i = -42_i64;
    assert_eq!(DragNum::from(&mut i).edit_string(), "-42");

    // Unparseable text leaves the value untouched.
    let mut i = 9_i64;
    assert!(!DragNum::from(&mut i).parse_from("12x", -INF, INF));
    assert_eq!(i, 9);
    assert!(DragNum::from(&mut i).parse_from("15", -INF, INF));
    assert_eq!(i, 15);

    // Non-finite parses are rejected even unbounded: a committed NaN would poison every scrub.
    let mut f = 7.5_f64;
    for bad in ["nan", "NaN", "inf", "-inf", "infinity"] {
        assert!(!DragNum::from(&mut f).parse_from(bad, -INF, INF), "{bad}");
        assert_eq!(f, 7.5, "{bad} must not land");
    }

    let mut i = 0_i64;
    assert!(DragNum::from(&mut i).parse_from("500", 0.0, 100.0));
    assert_eq!(i, 100);
    let mut f = 0.0_f64;
    assert!(!DragNum::from(&mut f).parse_from("-3.5", 0.0, 1.0));
    assert_eq!(f, 0.0);
    // A typed "-0.0" stores as +0.0.
    let mut f = 1.0_f64;
    assert!(DragNum::from(&mut f).parse_from("-0.0", -INF, INF));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits());
}
