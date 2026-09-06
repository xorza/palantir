//! Rounding, clamping, and the string round-trip both numeric variants
//! take.

use crate::widgets::drag_num::{DragNum, Num, round_to_decimals};

const INF: f64 = f64::INFINITY;

#[test]
fn round_to_decimals_snaps_and_formats_short() {
    // The reported long value snaps to its 3-decimal display and prints
    // without a tail — that's the whole point (edit_string shows this).
    let r = round_to_decimals(1.984_573_845_634_985_2, 3);
    assert_eq!(r, 1.985);
    assert_eq!(format!("{r:?}"), "1.985");
    // Fewer / zero decimals.
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 2), 1.98);
    assert_eq!(round_to_decimals(1.984_573_845_634_985_2, 0), 2.0);
    // Classic float-noise inputs collapse to a clean short value.
    assert_eq!(format!("{:?}", round_to_decimals(0.1 + 0.2, 1)), "0.3");
    assert_eq!(round_to_decimals(12.3456, 2), 12.35);
    // Negative values keep their sign.
    assert_eq!(round_to_decimals(-1.6789, 1), -1.7);
}

/// Past 2^53 an `f64` has no fractional digits left to round, and the
/// shift that would round them overflows — `1e308` at 2 decimals shifts
/// to infinity. Those magnitudes pass through: a rounding step must not
/// turn a finite value into an infinite one.
#[test]
fn round_to_decimals_leaves_an_unshiftable_magnitude_alone() {
    for v in [1e308, -1e308, f64::MAX, 1e17, -1e17] {
        for decimals in [0, 2, 15] {
            assert_eq!(round_to_decimals(v, decimals), v, "{v} at {decimals}");
        }
    }
    // The shift is skipped only where it has nothing to round: 2^53
    // itself is whole-only, while a half below it still rounds up.
    let whole_only = 9_007_199_254_740_992.0;
    assert_eq!(round_to_decimals(whole_only, 0), whole_only);
    assert_eq!(round_to_decimals(0.5, 0), 1.0);
    // NaN carries through as it did through the shift.
    assert!(round_to_decimals(f64::NAN, 2).is_nan());
}

#[test]
fn commit_drag_snaps_rounds_clamps_and_reports_change() {
    // Float: snaps to `decimals`, unbounded is a no-op clamp; the write
    // reports the change. The travel does the moving, so the same anchor
    // and the same offset land on the same value twice.
    let mut f = 0.0;
    let zero = Num::F64(0.0);
    assert!(DragNum::from(&mut f).commit_drag(zero, 1.984_573_845_634_985_2, 3, -INF, INF));
    assert_eq!(f, 1.985);
    // Re-committing the same scrub is a no-change write.
    assert!(!DragNum::from(&mut f).commit_drag(zero, 1.984_573_845_634_985_2, 3, -INF, INF));
    // A float anchor moves by its offset before the snap.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(Num::F64(1.5), 0.25, 2, -INF, INF));
    assert_eq!(f, 1.75);
    // Float: clamps into the range.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_value(50.0, 2, 0.0, 10.0));
    assert_eq!(f, 10.0);
    // A tiny negative wiggle at a 0.0 bound rounds to -0.0; the stored
    // value must be normalized to +0.0 (bit-exact) with no change report.
    let mut f = 0.0;
    assert!(!DragNum::from(&mut f).commit_value(-0.004, 2, 0.0, 1.0));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits(), "-0.0 normalized to +0.0");
    // Int: rounds to whole (decimals ignored), unbounded no-op clamp.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_value(7.6, 3, -INF, INF));
    assert_eq!(i, 8);
    assert!(!DragNum::from(&mut i).commit_value(7.6, 3, -INF, INF));
    // Int: clamps into the range.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_value(500.0, 0, 0.0, 100.0));
    assert_eq!(i, 100);
}

/// A float drag that reaches a magnitude with no room for `decimals`
/// keeps the value it reached: the snap is skipped, not overflowed. A
/// finite value the keyboard path accepts stays finite through a scrub.
#[test]
fn a_float_scrub_never_stores_an_infinite_value() {
    let mut f = 1e308_f64;
    assert!(!DragNum::from(&mut f).commit_drag(Num::F64(1e308), 0.0, 2, -INF, INF));
    assert_eq!(f, 1e308);
    // Moving it further stays finite, and the sum stands exactly as it
    // came out — at this magnitude there is no decimal left to snap to.
    assert!(DragNum::from(&mut f).commit_drag(Num::F64(1e308), 1e307, 2, -INF, INF));
    assert_eq!(f, 1e308 + 1e307);
}

/// An integer scrub moves the exact anchor, so the whole `i64` domain
/// survives it. Widening the anchor to `f64` first would round every
/// value past 2^53, and a drag that moves the value nowhere would store
/// that rounded number over the exact one typed entry accepted.
#[test]
fn an_integer_scrub_moves_the_exact_anchor() {
    let mut i = 9_007_199_254_740_993_i64;
    let anchor = DragNum::from(&mut i).read();
    assert_eq!(anchor, Num::I64(9_007_199_254_740_993));
    // Zero speed over any travel: no step, no write, no change report.
    assert!(!DragNum::from(&mut i).commit_drag(anchor, 0.0, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_993);
    // Half a step is no step; the value cannot drift on sub-step travel.
    assert!(!DragNum::from(&mut i).commit_drag(anchor, 0.4, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_993);
    // 2.4 steps of travel is two whole steps from that same anchor.
    assert!(DragNum::from(&mut i).commit_drag(anchor, 2.4, 2, -INF, INF));
    assert_eq!(i, 9_007_199_254_740_995);
    // Travel the domain cannot hold saturates rather than wrapping.
    assert!(DragNum::from(&mut i).commit_drag(anchor, INF, 2, -INF, INF));
    assert_eq!(i, i64::MAX);
    // A step off an anchor already at the far edge stays there.
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
    // Widening is for the visual quantities only, and says so by losing
    // the bit an `i64` past 2^53 carries.
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
    // Float keeps a trailing `.0` so it re-reads as a float, and a
    // fractional value survives verbatim (a same-value parse reports no
    // change).
    let mut f = 3.0_f64;
    assert_eq!(DragNum::from(&mut f).edit_string(), "3.0");
    let mut f = 2.5_f64;
    let s = DragNum::from(&mut f).edit_string();
    assert!(!DragNum::from(&mut f).parse_from(&s, -INF, INF));
    assert_eq!(f, 2.5);

    // Int formats and parses back exactly.
    let mut i = -42_i64;
    assert_eq!(DragNum::from(&mut i).edit_string(), "-42");

    // Unparseable text leaves the value untouched (partial input).
    let mut i = 9_i64;
    assert!(!DragNum::from(&mut i).parse_from("12x", -INF, INF));
    assert_eq!(i, 9);
    assert!(DragNum::from(&mut i).parse_from("15", -INF, INF));
    assert_eq!(i, 15);

    // Non-finite parses are rejected even unbounded — a committed NaN
    // would survive clamp and poison every subsequent scrub.
    let mut f = 7.5_f64;
    for bad in ["nan", "NaN", "inf", "-inf", "infinity"] {
        assert!(!DragNum::from(&mut f).parse_from(bad, -INF, INF), "{bad}");
        assert_eq!(f, 7.5, "{bad} must not land");
    }

    // Typed entry clamps into the range too.
    let mut i = 0_i64;
    assert!(DragNum::from(&mut i).parse_from("500", 0.0, 100.0));
    assert_eq!(i, 100);
    let mut f = 0.0_f64;
    assert!(!DragNum::from(&mut f).parse_from("-3.5", 0.0, 1.0));
    assert_eq!(f, 0.0);
    // A typed "-0.0" stores as +0.0 — sign-of-zero never leaks.
    let mut f = 1.0_f64;
    assert!(DragNum::from(&mut f).parse_from("-0.0", -INF, INF));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits());
}
