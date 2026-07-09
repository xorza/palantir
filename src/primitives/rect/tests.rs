use crate::primitives::rect::Rect;

#[test]
fn intersects_cases() {
    // Touching edges are NOT an intersection — DamageEngine filter leans on this so
    // a node whose left edge sits exactly on the damage rect's right edge gets
    // skipped.
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
    ];
    for (label, a, b, want) in cases {
        assert_eq!(a.union(*b), *want, "case: {label}");
        assert_eq!(b.union(*a), *want, "case: {label} (swapped)");
    }
}

/// `union_nonempty` treats paint-empty rects as identity elements —
/// folding a `Rect::ZERO`-seeded extent must not drag the accumulator
/// to the origin.
#[test]
fn union_nonempty_cases() {
    let far = Rect::new(5000.0, 5000.0, 30.0, 30.0);
    let other = Rect::new(5100.0, 5100.0, 10.0, 10.0);
    let cases: &[(&str, Rect, Rect, Rect)] = &[
        ("zero_left_is_identity", Rect::ZERO, far, far),
        ("zero_right_is_identity", far, Rect::ZERO, far),
        (
            "both_empty_returns_self",
            Rect::ZERO,
            Rect::ZERO,
            Rect::ZERO,
        ),
        (
            "sub_eps_sliver_is_identity",
            Rect::new(0.0, 0.0, 0.00005, 100.0),
            far,
            far,
        ),
        (
            "non_empty_pair_is_plain_union",
            far,
            other,
            Rect::new(5000.0, 5000.0, 110.0, 110.0),
        ),
    ];
    for (label, a, b, want) in cases {
        assert_eq!(a.union_nonempty(*b), *want, "case: {label}");
    }
    // Contrast with plain `union`, which origin-anchors: the exact bias
    // `union_nonempty` exists to avoid.
    assert_eq!(Rect::ZERO.union(far), Rect::new(0.0, 0.0, 5030.0, 5030.0));
}
