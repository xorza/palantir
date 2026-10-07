use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use glam::Vec2;

#[test]
fn from_min_max_preserves_extents() {
    assert_eq!(
        Rect::from_min_max(Vec2::new(-4.0, 7.0), Vec2::new(11.0, 19.0)),
        Rect::new(-4.0, 7.0, 15.0, 12.0),
    );
    assert_eq!(
        Rect::from_min_max(Vec2::new(3.0, 5.0), Vec2::new(3.0, 5.0)),
        Rect::new(3.0, 5.0, 0.0, 0.0),
    );
}

#[test]
fn intersects_cases() {
    // Touching edges are not an intersection; the DamageEngine filter relies on it.
    let cases: &[(&str, Rect, Rect, bool)] = &[
        (
            "overlapping",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 5.0, 10.0, 10.0),
            true,
        ),
        (
            "disjoint",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(20.0, 20.0, 5.0, 5.0),
            false,
        ),
        (
            "touching_edges",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(10.0, 0.0, 10.0, 10.0),
            false,
        ),
        (
            "self_with_self",
            Rect::new(2.0, 3.0, 4.0, 5.0),
            Rect::new(2.0, 3.0, 4.0, 5.0),
            true,
        ),
        (
            "zero_sized",
            Rect::new(0.0, 0.0, 0.0, 0.0),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            false,
        ),
    ];
    for (label, a, b, want) in cases {
        assert_eq!(a.intersects(*b), *want, "case: {label}");
        assert_eq!(b.intersects(*a), *want, "case: {label} (swapped)");
        // `intersect` is `Some` exactly when `intersects` is true and agrees with `clamp_to`; a miss is `None`.
        assert_eq!(a.intersect(*b).is_some(), *want, "strict: {label}");
        match a.intersect(*b) {
            Some(overlap) => assert_eq!(overlap, a.clamp_to(*b), "agree: {label}"),
            None => assert!(a.clamp_to(*b).is_paint_empty(), "saturating: {label}"),
        }
    }

    // Clamping keeps the overlap's origin, so it is not symmetric.
    let inner = Rect::new(2.0, 3.0, 4.0, 5.0);
    let outer = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(inner.clamp_to(outer), inner);
    assert_eq!(outer.clamp_to(inner), inner);

    let away = Rect::new(50.0, 50.0, 5.0, 5.0);
    let missed = outer.clamp_to(away);
    assert!(missed.is_paint_empty(), "{missed:?}");
    // `clamp_to` takes the bounds' origin and the smaller max, saturating both extents at zero.
    assert_eq!(missed, Rect::new(50.0, 50.0, 0.0, 0.0));
}

/// A NaN operand is a broken contract, not a miss (`f32::max` would drop it).
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "probes a debug_assert!, which release compiles out"
)]
fn a_nan_operand_is_refused() {
    for (a, b) in [
        (Rect::NAN, Rect::new(0.0, 0.0, 10.0, 10.0)),
        (Rect::new(0.0, 0.0, 10.0, 10.0), Rect::NAN),
    ] {
        panic_probe::assert_panics_with("NaN operand", || a.intersect(b));
        panic_probe::assert_panics_with("NaN operand", || a.clamp_to(b));
    }
}

#[test]
fn union_cases() {
    let cases: &[(&str, Rect, Rect, Rect)] = &[
        (
            "overlapping_returns_bounding_box",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 5.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 15.0, 15.0),
        ),
        (
            "disjoint_returns_enclosing_rect",
            Rect::new(0.0, 0.0, 5.0, 5.0),
            Rect::new(10.0, 10.0, 5.0, 5.0),
            Rect::new(0.0, 0.0, 15.0, 15.0),
        ),
        (
            "with_self_is_self",
            Rect::new(2.0, 3.0, 4.0, 5.0),
            Rect::new(2.0, 3.0, 4.0, 5.0),
            Rect::new(2.0, 3.0, 4.0, 5.0),
        ),
        (
            "commutative",
            Rect::new(1.0, 2.0, 3.0, 4.0),
            Rect::new(7.0, 8.0, 5.0, 6.0),
            Rect::new(1.0, 2.0, 11.0, 12.0),
        ),
        // Paint-empty operands are identity elements, as in `URect::union`.
        (
            "paint_empty_is_identity",
            Rect::ZERO,
            Rect::new(5000.0, 5000.0, 30.0, 30.0),
            Rect::new(5000.0, 5000.0, 30.0, 30.0),
        ),
        (
            "sub_eps_sliver_is_identity",
            Rect::new(0.0, 0.0, 0.00005, 100.0),
            Rect::new(5000.0, 5000.0, 30.0, 30.0),
            Rect::new(5000.0, 5000.0, 30.0, 30.0),
        ),
        (
            "both_empty_returns_left",
            Rect::ZERO,
            Rect::ZERO,
            Rect::ZERO,
        ),
    ];
    for (label, a, b, want) in cases {
        assert_eq!(a.union(*b), *want, "case: {label}");
        assert_eq!(b.union(*a), *want, "case: {label} (swapped)");
    }
}

/// Inset/outset spellings against hand-computed rects; an outset cannot collapse a rect, an inset clamps at zero.
#[test]
fn inflate_and_deflate_are_inverses_until_the_clamp() {
    let r = Rect::new(10.0, 20.0, 100.0, 40.0);

    assert_eq!(r.inflated(2.0), Rect::new(8.0, 18.0, 104.0, 44.0));
    assert_eq!(r.deflated(2.0), Rect::new(12.0, 22.0, 96.0, 36.0));
    assert_eq!(r.inflated(2.0).deflated(2.0), r);

    let s = Spacing::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(r.inflated_by(s), Rect::new(9.0, 18.0, 104.0, 46.0));
    assert_eq!(r.deflated_by(s), Rect::new(11.0, 22.0, 96.0, 34.0));
    assert_eq!(r.deflated_by(s).inflated_by(s), r);
    assert_eq!(r.inflated(2.5).deflated_by(Spacing::all(2.5)), r);

    // 25 px off each side of a 40 px-tall rect wants -10: the inset clamps to zero.
    let flattened = r.deflated(25.0);
    assert_eq!(flattened, Rect::new(35.0, 45.0, 50.0, 0.0));
    assert_eq!(flattened.inflated(25.0), Rect::new(10.0, 20.0, 100.0, 50.0));
}

/// Each side insets by its larger adjacent radius times `1 − 1/√2` (the 45° point on the corner arc), so the
/// occlusion prune never drops an under-quad whose corner sits in the rounded cutout.
#[test]
fn inscribed_for_corners_insets_to_the_arc_midpoint() {
    let per_radius = 1.0 - 1.0 / 2.0_f32.sqrt();
    let square = Rect::new(0.0, 0.0, 100.0, 100.0);
    let uniform = 10.0 * per_radius;
    let one = 20.0 * per_radius;
    for (label, rect, corners, inscribed) in [
        (
            "uniform 10",
            square,
            Corners::all(10.0),
            Rect::new(
                uniform,
                uniform,
                100.0 - 2.0 * uniform,
                100.0 - 2.0 * uniform,
            ),
        ),
        (
            "sharp",
            Rect::new(5.0, 10.0, 30.0, 40.0),
            Corners::ZERO,
            Rect::new(5.0, 10.0, 30.0, 40.0),
        ),
        (
            "top-left 20",
            square,
            Corners::new(20.0, 0.0, 0.0, 0.0),
            Rect::new(one, one, 100.0 - one, 100.0 - one),
        ),
    ] {
        assert_eq!(rect.inscribed_for_corners(corners), inscribed, "{label}");
    }
}

/// Logical to physical, with and without pixel snap (inputs are multiples of 1/4, so scaled edges are
/// exact). Snapped, size comes from the rounded edges: 31 × 8, not the 30.75 × 7.875 a scaled size would
/// round to; a rect thinner than half a pixel at both edges snaps to nothing.
#[test]
fn scaled_by_rounds_edges_only_when_snapping() {
    let r = Rect::new(10.25, 10.75, 20.5, 5.25);
    assert_eq!(
        r.scaled_by(1.5, false),
        Rect::new(15.375, 16.125, 30.75, 7.875)
    );
    assert_eq!(r.scaled_by(1.5, true), Rect::new(15.0, 16.0, 31.0, 8.0));

    let sliver = Rect::new(10.125, 0.0, 0.25, 1.0);
    assert_eq!(sliver.scaled_by(1.0, false).size.w, 0.25);
    assert_eq!(sliver.scaled_by(1.0, true), Rect::new(10.0, 0.0, 0.0, 1.0));
}
