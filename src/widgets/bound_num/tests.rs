//! Rounding, clamping, and the string round-trip every numeric variant takes.

use crate::widgets::bound_num::{BoundNum, Num, StoreRule, round_to_decimals};

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
    let mut f = 0.0_f64;
    let zero = Num::F64(0.0);
    assert!(BoundNum::from(&mut f).commit_drag(
        zero,
        1.984_573_845_634_985_2,
        StoreRule::new(-INF, INF, 3)
    ));
    assert_eq!(f, 1.985);
    assert!(!BoundNum::from(&mut f).commit_drag(
        zero,
        1.984_573_845_634_985_2,
        StoreRule::new(-INF, INF, 3)
    ));
    let mut f = 0.0_f64;
    assert!(BoundNum::from(&mut f).commit_drag(Num::F64(1.5), 0.25, StoreRule::new(-INF, INF, 2)));
    assert_eq!(f, 1.75);
    let mut f = 0.0_f64;
    assert!(BoundNum::from(&mut f).commit_value(50.0, StoreRule::new(0.0, 10.0, 2)));
    assert_eq!(f, 10.0);
    // A tiny negative wiggle at a 0.0 bound is stored as +0.0 (bit-exact) with no change report.
    let mut f = 0.0_f64;
    assert!(!BoundNum::from(&mut f).commit_value(-0.004, StoreRule::new(0.0, 1.0, 2)));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits(), "-0.0 normalized to +0.0");
    let mut i = 0_i64;
    assert!(BoundNum::from(&mut i).commit_value(7.6, StoreRule::new(-INF, INF, 3)));
    assert_eq!(i, 8);
    assert!(!BoundNum::from(&mut i).commit_value(7.6, StoreRule::new(-INF, INF, 3)));
    let mut i = 0_i64;
    assert!(BoundNum::from(&mut i).commit_value(500.0, StoreRule::new(0.0, 100.0, 0)));
    assert_eq!(i, 100);
    // Int: a fractional bound closes in to the integers inside it (0.5..=10 holds no 0).
    let mut i = 5_i64;
    assert!(BoundNum::from(&mut i).commit_value(-3.0, StoreRule::new(0.5, 10.0, 0)));
    assert_eq!(i, 1);
    let mut i = -5_i64;
    assert!(BoundNum::from(&mut i).commit_value(3.0, StoreRule::new(-10.0, -0.5, 0)));
    assert_eq!(i, -1);
    // A range holding no integer clamps between the two either side.
    let mut i = 5_i64;
    assert!(BoundNum::from(&mut i).commit_value(5.0, StoreRule::new(0.2, 0.8, 0)));
    assert_eq!(i, 1);
}

/// A float drag at a magnitude with no room for `decimals` keeps the value it reached: the snap is skipped, not overflowed.
#[test]
fn a_float_scrub_never_stores_an_infinite_value() {
    let mut f = 1e308_f64;
    assert!(!BoundNum::from(&mut f).commit_drag(
        Num::F64(1e308),
        0.0,
        StoreRule::new(-INF, INF, 2)
    ));
    assert_eq!(f, 1e308);
    // Moving further stays finite and the sum stands exactly.
    assert!(BoundNum::from(&mut f).commit_drag(
        Num::F64(1e308),
        1e307,
        StoreRule::new(-INF, INF, 2)
    ));
    assert_eq!(f, 1e308 + 1e307);
}

/// An integer scrub moves the exact anchor, so the whole `i64` domain survives; widening to `f64` first would round values past 2^53.
#[test]
fn an_integer_scrub_moves_the_exact_anchor() {
    let mut i = 9_007_199_254_740_993_i64;
    let anchor = BoundNum::from(&mut i).read();
    assert_eq!(anchor, Num::I64(9_007_199_254_740_993));
    assert!(!BoundNum::from(&mut i).commit_drag(anchor, 0.0, StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, 9_007_199_254_740_993);
    assert!(!BoundNum::from(&mut i).commit_drag(anchor, 0.4, StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, 9_007_199_254_740_993);
    assert!(BoundNum::from(&mut i).commit_drag(anchor, 2.4, StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, 9_007_199_254_740_995);
    // Travel the domain cannot hold saturates rather than wrapping.
    assert!(BoundNum::from(&mut i).commit_drag(anchor, INF, StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, i64::MAX);
    let mut edge = i64::MIN;
    assert!(!BoundNum::from(&mut edge).commit_drag(
        Num::I64(i64::MIN),
        -3.0,
        StoreRule::new(-INF, INF, 2)
    ));
    assert_eq!(edge, i64::MIN);
}

/// A reversed pair is the same range; a degenerate pair collapses to its point; an infinite one
/// passes every finite value; a NaN bound reports the other end, so the clamp pins to a number.
#[test]
fn a_store_rule_orders_its_bounds_and_stays_usable() {
    for (min, max) in [(0.0, 10.0), (10.0, 0.0)] {
        for (value, want) in [
            (5.0, 5.0),
            (-1.0, 0.0),
            (11.0, 10.0),
            (0.0, 0.0),
            (10.0, 10.0),
        ] {
            assert_eq!(
                StoreRule::new(min, max, 2).clamp(value),
                want,
                "{min}..={max} at {value}"
            );
        }
    }
    assert_eq!(StoreRule::new(3.0, 3.0, 2).clamp(9.0), 3.0);
    let unbounded = StoreRule::new(-INF, INF, 2);
    assert_eq!(unbounded.clamp(-1.0e300), -1.0e300);
    assert_eq!(unbounded.clamp(1.0e300), 1.0e300);
    let nan_bound = StoreRule::new(f64::NAN, 4.0, 2);
    assert_eq!((nan_bound.lo, nan_bound.hi), (4.0, 4.0));
    assert_eq!(nan_bound.clamp(100.0), 4.0);
}

/// A non-finite float binding anchors a gesture at the neutral `0.0`, so a scrub of `+1.5` from
/// NaN stores `1.5` where it stored `-inf`, and from `+inf` stores `1.5` where it stuck at `+inf`;
/// [`BoundNum::stored`] still reads the bits a canceled edit writes back.
#[test]
fn bound_num_read_keeps_precision_and_anchors_a_non_finite_float_at_zero() {
    for bound in [f64::NAN, INF, -INF] {
        let mut f = bound;
        assert_eq!(BoundNum::from(&mut f).read(), Num::F64(0.0), "{bound}");
        let Num::F64(stored) = BoundNum::from(&mut f).stored() else {
            unreachable!("a float binding reads a float")
        };
        assert_eq!(stored.to_bits(), bound.to_bits(), "{bound}");
        let anchor = BoundNum::from(&mut f).read();
        assert!(BoundNum::from(&mut f).commit_drag(anchor, 1.5, StoreRule::new(-INF, INF, 2)));
        assert_eq!(f, 1.5, "{bound}");
    }

    let mut f = 2.5_f64;
    assert_eq!(BoundNum::from(&mut f).read(), Num::F64(2.5));
    let mut i = 5_i64;
    assert_eq!(BoundNum::from(&mut i).read(), Num::I64(5));
    // Widening is for visual quantities only and loses the low bit of an `i64` past 2^53.
    let mut i = 9_007_199_254_740_993_i64;
    assert_eq!(
        BoundNum::from(&mut i).read(),
        Num::I64(9_007_199_254_740_993)
    );
    assert_eq!(
        BoundNum::from(&mut i).read().widen(),
        9_007_199_254_740_992.0
    );
}

#[test]
fn bound_num_edit_string_and_parse_round_trip() {
    // Float keeps a trailing `.0` so it re-reads as a float; a fractional value survives verbatim.
    let mut f = 3.0_f64;
    assert_eq!(BoundNum::from(&mut f).edit_string(), "3.0");
    let mut f = 2.5_f64;
    let s = BoundNum::from(&mut f).edit_string();
    assert!(!BoundNum::from(&mut f).parse_from(&s, StoreRule::new(-INF, INF, 2)));
    assert_eq!(f, 2.5);

    let mut i = -42_i64;
    assert_eq!(BoundNum::from(&mut i).edit_string(), "-42");

    // Unparseable text leaves the value untouched.
    let mut i = 9_i64;
    assert!(!BoundNum::from(&mut i).parse_from("12x", StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, 9);
    assert!(BoundNum::from(&mut i).parse_from("15", StoreRule::new(-INF, INF, 2)));
    assert_eq!(i, 15);

    // Non-finite parses are rejected even unbounded: a committed NaN would poison every scrub.
    let mut f = 7.5_f64;
    for bad in ["nan", "NaN", "inf", "-inf", "infinity"] {
        assert!(
            !BoundNum::from(&mut f).parse_from(bad, StoreRule::new(-INF, INF, 2)),
            "{bad}"
        );
        assert_eq!(f, 7.5, "{bad} must not land");
    }

    let mut i = 0_i64;
    assert!(BoundNum::from(&mut i).parse_from("500", StoreRule::new(0.0, 100.0, 2)));
    assert_eq!(i, 100);
    let mut f = 0.0_f64;
    assert!(!BoundNum::from(&mut f).parse_from("-3.5", StoreRule::new(0.0, 1.0, 2)));
    assert_eq!(f, 0.0);
    // A typed "-0.0" stores as +0.0.
    let mut f = 1.0_f64;
    assert!(BoundNum::from(&mut f).parse_from("-0.0", StoreRule::new(-INF, INF, 2)));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits());
}

/// The 32-bit targets share their wider twins' rules: an `i32` scrub past its range stops at
/// `i32::MAX` whatever the unbounded rule, a parse reads it back, and a restore writes the
/// opening value; an `f32` snaps in `f64`, `0.1` at one decimal, then stores the nearest `f32`,
/// whose bits differ from the `f64`'s, and restores bit for bit.
#[test]
fn the_32_bit_targets_store_into_their_own_width() {
    let unbounded = StoreRule::new(-INF, INF, 1);
    let mut i = i32::MAX - 1;
    let opening = BoundNum::from(&mut i).stored();
    assert!(BoundNum::from(&mut i).commit_drag(opening, 10.0, unbounded));
    assert_eq!(i, i32::MAX);
    assert!(BoundNum::from(&mut i).parse_from("-7", unbounded));
    assert_eq!(i, -7);
    assert!(BoundNum::from(&mut i).restore(opening));
    assert_eq!(i, i32::MAX - 1);

    let mut f = 0.0_f32;
    assert!(BoundNum::from(&mut f).commit_value(0.123, unbounded));
    assert_eq!(f, 0.1_f32);
    assert_ne!(
        f64::from(f).to_bits(),
        0.1_f64.to_bits(),
        "an f32 holds no exact 0.1"
    );
    let snapped = BoundNum::from(&mut f).stored();
    assert_eq!(snapped, Num::F64(f64::from(0.1_f32)));
    assert!(BoundNum::from(&mut f).commit_value(2.0, unbounded));
    assert!(BoundNum::from(&mut f).restore(snapped));
    assert_eq!(f.to_bits(), 0.1_f32.to_bits());
    assert_eq!(BoundNum::from(&mut f).edit_string(), "0.1");
}

/// A step grid runs from `min`, or from zero while `min` is unbounded, and stays inside the
/// bounds.
#[test]
fn a_step_grid_snaps_from_min_inside_the_bounds() {
    let grid = |min: f64, max: f64, step: f64| StoreRule::new(min, max, 2).step(Some(step));
    assert_eq!(grid(0.0, 100.0, 10.0).snapped(53.0), 50.0);
    assert_eq!(grid(0.0, 100.0, 10.0).snapped(57.0), 60.0);
    assert_eq!(grid(0.0, 100.0, 5.0).snapped(12.0), 10.0);
    assert_eq!(grid(0.0, 100.0, 5.0).snapped(13.0), 15.0);
    assert_eq!(grid(1.0, 100.0, 0.5).snapped(2.2), 2.0);
    assert_eq!(StoreRule::new(0.0, 100.0, 2).snapped(53.0), 53.0);

    // `0..=10` by 4 holds 0, 4 and 8, so 10 and 11 land on 8 (rounding alone gives 12) and -3
    // on 0; reversed, `10..=0` by 4 holds 10, 6 and 2.
    assert_eq!(grid(0.0, 10.0, 4.0).snapped(10.0), 8.0);
    assert_eq!(grid(0.0, 10.0, 4.0).snapped(11.0), 8.0);
    assert_eq!(grid(0.0, 10.0, 4.0).snapped(-3.0), 0.0);
    assert_eq!(grid(10.0, 0.0, 4.0).snapped(-1.0), 2.0);
    assert_eq!(grid(10.0, 0.0, 4.0).snapped(11.0), 10.0);
    // `0.3 / 0.1` divides to just under 3, and 0.3 is still the third step, not the second.
    assert_eq!(grid(0.0, 0.3, 0.1).snapped(0.3), 3.0 * 0.1);
    // That allowance is an ulp of the span, not of the count: half a step past `1e12` whole
    // steps is no whole count, so the last grid value is `1e12`, not `1e12 + 1` past the end.
    assert_eq!(grid(0.0, 1e12 + 0.5, 1.0).snapped(2e12), 1e12);

    // Unbounded below, the grid runs from zero: by 0.5, 1.3 lands on 1.5; under a max of 7 by
    // 2, 9 lands on 6, the last grid value inside.
    assert_eq!(grid(-INF, INF, 0.5).snapped(1.3), 1.5);
    assert_eq!(grid(-INF, 7.0, 2.0).snapped(9.0), 6.0);
    assert_eq!(grid(-INF, 7.0, 2.0).snapped(-9.2), -10.0);
}

/// Every write lands on the grid: a scrub of an integer by 2.6 from 10 under a grid of 5 is
/// 12.6, nearest 15; a typed 23 is 25 and a typed 2.4 on a float is 2.5.
#[test]
fn every_write_lands_on_the_step_grid() {
    let by_five = StoreRule::new(0.0, 100.0, 2).step(Some(5.0));
    let mut i = 10_i64;
    assert!(BoundNum::from(&mut i).commit_drag(Num::I64(10), 2.6, by_five));
    assert_eq!(i, 15);
    assert!(BoundNum::from(&mut i).parse_from("23", by_five));
    assert_eq!(i, 25);
    assert!(
        !BoundNum::from(&mut i).parse_from("2.5", by_five),
        "an integer parses whole"
    );
    assert_eq!(i, 25);

    let mut f = 0.0_f64;
    let by_half = StoreRule::new(-INF, INF, 2).step(Some(0.5));
    assert!(BoundNum::from(&mut f).parse_from("2.4", by_half));
    assert_eq!(f, 2.5);
    assert!(!BoundNum::from(&mut f).parse_from("nan", by_half));
    assert_eq!(f, 2.5);
}
