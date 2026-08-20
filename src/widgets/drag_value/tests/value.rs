//! Rounding, clamping, and the string round-trip both numeric variants
//! take.

use crate::widgets::drag_value::{DragNum, round_to_decimals};

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

#[test]
fn commit_drag_snaps_rounds_clamps_and_reports_change() {
    const INF: f64 = f64::INFINITY;
    // Float: snaps to `decimals`, unbounded is a no-op clamp; the write
    // reports the change.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(1.984_573_845_634_985_2, 3, -INF, INF));
    assert_eq!(f, 1.985);
    // Re-committing the same raw is a no-change write.
    assert!(!DragNum::from(&mut f).commit_drag(1.984_573_845_634_985_2, 3, -INF, INF));
    // Float: clamps into the range.
    let mut f = 0.0;
    assert!(DragNum::from(&mut f).commit_drag(50.0, 2, 0.0, 10.0));
    assert_eq!(f, 10.0);
    // A tiny negative wiggle at a 0.0 bound rounds to -0.0; the stored
    // value must be normalized to +0.0 (bit-exact) with no change report.
    let mut f = 0.0;
    assert!(!DragNum::from(&mut f).commit_drag(-0.004, 2, 0.0, 1.0));
    assert_eq!(f.to_bits(), 0.0_f64.to_bits(), "-0.0 normalized to +0.0");
    // Int: rounds to whole (decimals ignored), unbounded no-op clamp.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_drag(7.6, 3, -INF, INF));
    assert_eq!(i, 8);
    assert!(!DragNum::from(&mut i).commit_drag(7.6, 3, -INF, INF));
    // Int: clamps into the range.
    let mut i = 0;
    assert!(DragNum::from(&mut i).commit_drag(500.0, 0, 0.0, 100.0));
    assert_eq!(i, 100);
}

#[test]
fn drag_num_get_reads_both_variants() {
    let mut f = 2.5_f64;
    assert_eq!(DragNum::from(&mut f).get(), 2.5);
    let mut i = 5_i64;
    assert_eq!(DragNum::from(&mut i).get(), 5.0);
}

#[test]
fn drag_num_edit_string_and_parse_round_trip() {
    const INF: f64 = f64::INFINITY;
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
