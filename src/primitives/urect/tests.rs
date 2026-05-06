use super::URect;

#[test]
fn intersect_cases() {
    // Strict overlap: touching edges return None. Mirror of `Rect::intersects`.
    let cases: &[(&str, URect, URect, Option<URect>)] = &[
        (
            "overlapping",
            URect::new(0, 0, 10, 10),
            URect::new(5, 5, 10, 10),
            Some(URect::new(5, 5, 5, 5)),
        ),
        (
            "disjoint",
            URect::new(0, 0, 10, 10),
            URect::new(20, 20, 5, 5),
            None,
        ),
        (
            "touching_edges",
            URect::new(0, 0, 10, 10),
            URect::new(10, 0, 10, 10),
            None,
        ),
        (
            "contained",
            URect::new(0, 0, 100, 100),
            URect::new(20, 30, 10, 10),
            Some(URect::new(20, 30, 10, 10)),
        ),
        (
            "self_with_self",
            URect::new(5, 7, 11, 13),
            URect::new(5, 7, 11, 13),
            Some(URect::new(5, 7, 11, 13)),
        ),
    ];
    for (label, a, b, want) in cases {
        assert_eq!(a.intersect(*b), *want, "case: {label}");
        assert_eq!(b.intersect(*a), *want, "case: {label} (swapped)");
    }
}

#[test]
fn clamp_to_cases() {
    let cases: &[(&str, URect, URect, URect)] = &[
        (
            "inside_parent_returns_self",
            URect::new(20, 30, 10, 10),
            URect::new(0, 0, 100, 100),
            URect::new(20, 30, 10, 10),
        ),
        (
            "overlapping_parent_clips_to_overlap",
            URect::new(40, 40, 30, 30),
            URect::new(0, 0, 50, 50),
            URect::new(40, 40, 10, 10),
        ),
        (
            "disjoint_returns_zero_sized",
            URect::new(20, 20, 5, 5),
            URect::new(0, 0, 10, 10),
            URect::new(20, 20, 0, 0),
        ),
    ];
    for (label, me, parent, want) in cases {
        let got = me.clamp_to(*parent);
        assert_eq!(got.w, want.w, "case: {label} w");
        assert_eq!(got.h, want.h, "case: {label} h");
        if got.w != 0 && got.h != 0 {
            assert_eq!(got, *want, "case: {label}");
        }
    }
}
