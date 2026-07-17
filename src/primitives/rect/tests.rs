use crate::primitives::rect::Rect;
use glam::Vec2;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_value(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn equal_signed_zero_rects_have_equal_hashes() {
    let positive = Rect::new(0.0, 0.0, 0.0, 0.0);
    let negative = Rect::new(-0.0, -0.0, -0.0, -0.0);

    assert_eq!(positive, negative);
    assert_eq!(hash_value(positive), hash_value(negative));
}

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
        // Paint-empty operands are identity elements (same contract as
        // `URect::union`) — folding a `Rect::ZERO`-seeded extent must
        // not drag the accumulator's min to the origin.
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
