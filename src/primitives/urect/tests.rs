use crate::primitives::urect::URect;

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
        assert_eq!(got.size.x, want.size.x, "case: {label} w");
        assert_eq!(got.size.y, want.size.y, "case: {label} h");
        if got.size.x != 0 && got.size.y != 0 {
            assert_eq!(got, *want, "case: {label}");
        }
    }
}

/// The two rectangles answer the same questions the same way.
///
/// The point of the pair is that a reader who knows one knows the other, and
/// the way that decays is quietly: a method that means something subtly
/// different under the same name is worse than one that is missing. So the
/// shared vocabulary is checked against [`Rect`] on the same coordinates
/// rather than each side being checked against hand-written answers.
///
/// Whole numbers throughout, which is what makes the comparison exact — the
/// float rect is being asked the integer rect's questions.
#[test]
fn the_two_rectangles_agree_on_the_vocabulary_they_share() {
    use crate::primitives::rect::Rect;
    use glam::UVec2;

    let pairs = [
        // overlapping, disjoint, touching, contained, and one empty.
        ((10u32, 10, 40, 30), (20u32, 15, 40, 30)),
        ((0, 0, 10, 10), (50, 50, 10, 10)),
        ((0, 0, 10, 10), (10, 0, 10, 10)),
        ((0, 0, 100, 100), (20, 30, 10, 10)),
        ((5, 5, 0, 0), (0, 0, 10, 10)),
    ];
    for (a, b) in pairs {
        let (ua, ub) = (
            URect::new(a.0, a.1, a.2, a.3),
            URect::new(b.0, b.1, b.2, b.3),
        );
        let (ra, rb) = (Rect::from(ua), Rect::from(ub));
        let label = format!("{a:?} vs {b:?}");

        assert_eq!(ua.is_paint_empty(), ra.is_paint_empty(), "empty: {label}");
        assert_eq!(ua.intersects(ub), ra.intersects(rb), "intersects: {label}");
        assert_eq!(
            ua.contains_rect(ub),
            ra.contains_rect(rb),
            "contains_rect: {label}"
        );
        assert_eq!(
            Rect::from(ua.clamp_to(ub)),
            ra.clamp_to(rb),
            "clamp: {label}"
        );
        assert_eq!(Rect::from(ua.union(ub)), ra.union(rb), "union: {label}");
        assert_eq!(
            ua.intersect(ub).map(Rect::from),
            ra.intersect(rb),
            "intersect: {label}"
        );
        assert_eq!(ua.max().as_vec2(), ra.max(), "max: {label}");
        assert_eq!(ua.area() as f32, ra.area(), "area: {label}");
        // Half-open on both, so the min corner is in and the max corner is out.
        assert!(ua.contains(ua.min) == ra.contains(ra.min) || ua.is_paint_empty());
        assert!(
            !ua.contains(ua.max()) && !ra.contains(ra.max()),
            "max in: {label}"
        );
    }

    // The conversions are inverse on whole pixels, which is what lets the
    // comparison above stand for anything.
    let r = URect::new(3, 7, 11, 13);
    assert_eq!(URect::covering(Rect::from(r)), r);
    // And covering rounds outward: a rect inside one pixel covers that pixel.
    assert_eq!(
        URect::covering(Rect::new(2.2, 3.8, 0.1, 0.1)),
        URect::new(2, 3, 1, 1),
        "a sliver has to cover the pixel it sits in"
    );
    // A rect reaching left of the origin starts at it, and keeps its far edge.
    assert_eq!(
        URect::covering(Rect::new(-5.0, -5.0, 10.0, 10.0)),
        URect::new(0, 0, 5, 5)
    );
    // Nothing finite, nothing covered.
    assert_eq!(URect::covering(Rect::NAN), URect::ZERO);

    // `from_min_max` is `new`'s other spelling, and saturates where a float
    // rect would debug-assert.
    assert_eq!(
        URect::from_min_max(UVec2::new(3, 7), UVec2::new(14, 20)),
        URect::new(3, 7, 11, 13)
    );
    assert_eq!(
        URect::from_min_max(UVec2::new(10, 10), UVec2::new(4, 4)),
        URect::new(10, 10, 0, 0),
        "an inverted pair is the empty rect at its own min"
    );

    // Inflating saturates at the origin rather than wrapping below it.
    assert_eq!(
        URect::new(10, 10, 5, 5).inflated(3),
        URect::new(7, 7, 11, 11)
    );
    assert_eq!(URect::new(1, 1, 5, 5).inflated(4), URect::new(0, 0, 13, 13));
}

/// The four `u32`s still sit in `x, y, w, h` order.
///
/// Load-bearing rather than pedantic: this type is `Pod`, it is hashed through
/// [`bytemuck::bytes_of`], and its whole reason for storing origin + extent is
/// that it round-trips with wgpu's `set_scissor_rect(x, y, w, h)` without
/// arithmetic. Naming the halves `min` and `size` was meant to change how it
/// reads and nothing about how it lies in memory, and a reordering would be
/// invisible until a scissor came out transposed.
#[test]
fn the_fields_still_lie_in_scissor_order() {
    let r = URect::new(1, 2, 3, 4);
    assert_eq!(size_of::<URect>(), 16);
    assert_eq!(
        bytemuck::bytes_of(&r),
        &1u32
            .to_ne_bytes()
            .iter()
            .chain(2u32.to_ne_bytes().iter())
            .chain(3u32.to_ne_bytes().iter())
            .chain(4u32.to_ne_bytes().iter())
            .copied()
            .collect::<Vec<u8>>()[..]
    );
}
