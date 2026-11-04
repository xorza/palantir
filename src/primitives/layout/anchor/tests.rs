use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::anchor::{Anchor, AnchorAlign, AnchorSide};
use glam::Vec2;

const BOUNDS: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);
const RECT: Rect = Rect::new(160.0, 120.0, 80.0, 40.0);
const BODY: Size = Size::new(100.0, 60.0);

#[test]
fn each_preferred_side_places_outside_the_rect() {
    let cases = [
        (AnchorSide::Above, Vec2::new(160.0, 54.0)),
        (AnchorSide::Below, Vec2::new(160.0, 166.0)),
        (AnchorSide::LeftOf, Vec2::new(54.0, 120.0)),
        (AnchorSide::RightOf, Vec2::new(246.0, 120.0)),
    ];
    for (side, expected) in cases {
        let anchor = Anchor::on_side(RECT, side).with_gap(6.0);
        assert_eq!(anchor.resolve(BODY, BOUNDS), expected, "{side:?}");
    }
}

#[test]
fn overflowing_preferred_side_flips_across_the_rect() {
    let cases = [
        (
            Anchor::on_side(Rect::new(160.0, 4.0, 80.0, 40.0), AnchorSide::Above).with_gap(6.0),
            Vec2::new(160.0, 50.0),
        ),
        (
            Anchor::on_side(Rect::new(160.0, 256.0, 80.0, 40.0), AnchorSide::Below).with_gap(6.0),
            Vec2::new(160.0, 190.0),
        ),
        (
            Anchor::on_side(Rect::new(4.0, 120.0, 80.0, 40.0), AnchorSide::LeftOf).with_gap(6.0),
            Vec2::new(90.0, 120.0),
        ),
        (
            Anchor::on_side(Rect::new(316.0, 120.0, 80.0, 40.0), AnchorSide::RightOf).with_gap(6.0),
            Vec2::new(210.0, 120.0),
        ),
    ];
    for (anchor, expected) in cases {
        assert_eq!(anchor.resolve(BODY, BOUNDS), expected);
    }
}

#[test]
fn cross_axis_alignment_uses_the_full_rect() {
    let cases = [
        (AnchorAlign::Start, 160.0),
        (AnchorAlign::Center, 150.0),
        (AnchorAlign::End, 140.0),
    ];
    for (align, expected_x) in cases {
        let anchor = Anchor::below(RECT).with_align(align);
        assert_eq!(
            anchor.resolve(BODY, BOUNDS),
            Vec2::new(expected_x, 160.0),
            "{align:?}",
        );
    }
}

/// A body measures along its side against the room the roomier side leaves, and across it
/// against the whole bounds; a body that measured to that room lands on that side, clear of the
/// rect. Below a rect at y 220..260 with a gap of 6, `300 - 266 = 34` is left below and
/// `220 - 6 = 214` above; a 214 tall body lands at the top, `214 + 6` above the rect.
#[test]
fn a_body_too_long_for_both_sides_measures_to_the_roomier_one_and_lands_there() {
    let anchor = Anchor::below(Rect::new(160.0, 220.0, 80.0, 40.0)).with_gap(6.0);
    let available = anchor.available(BOUNDS);
    assert_eq!(available, Size::new(400.0, 214.0));
    assert_eq!(
        anchor.resolve(Size::new(100.0, available.h), BOUNDS),
        Vec2::new(160.0, 0.0)
    );
    // Across a horizontal side the room is a width: right of a rect ending at 240 leaves
    // `400 - 246 = 154`, left of it `160 - 6 = 154` too.
    let beside = Anchor::right_of(Rect::new(160.0, 120.0, 80.0, 40.0)).with_gap(6.0);
    assert_eq!(beside.available(BOUNDS), Size::new(154.0, 300.0));
    // A rect past an edge leaves no room there.
    let past = Anchor::above(Rect::new(0.0, -10.0, 10.0, 5.0));
    assert_eq!(past.room(AnchorSide::Above, BOUNDS), 0.0);
}

#[test]
fn impossible_fit_clamps_inside_bounds() {
    let anchor =
        Anchor::on_side(Rect::new(390.0, 140.0, 10.0, 20.0), AnchorSide::Below).with_gap(6.0);
    assert_eq!(anchor.resolve(Size::new(500.0, 400.0), BOUNDS), Vec2::ZERO,);
}

/// A point anchor is what `at_point` makes, whatever its rect: a rect that collapsed to no size
/// below a button stays a rect, and both resolve to the same origin.
#[test]
fn a_point_is_what_at_point_makes_not_an_empty_rect() {
    let point = Vec2::new(30.0, 40.0);
    let at_point = Anchor::at_point(point);
    let collapsed = Anchor::below(Rect::new(point.x, point.y, 0.0, 0.0));
    assert!(at_point.is_point());
    assert!(!collapsed.is_point());
    assert!(!Anchor::below(RECT).is_point());
    assert_ne!(at_point, collapsed);
    assert_eq!(
        at_point.resolve(BODY, BOUNDS),
        collapsed.resolve(BODY, BOUNDS)
    );
}
