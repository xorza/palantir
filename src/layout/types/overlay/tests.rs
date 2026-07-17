use crate::layout::types::align::AxisAlign;
use crate::layout::types::overlay::{OverlayPosition, OverlaySide};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

const BOUNDS: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);
const ANCHOR: Rect = Rect::new(160.0, 120.0, 80.0, 40.0);
const BODY: Size = Size::new(100.0, 60.0);

#[test]
fn each_preferred_side_places_outside_the_anchor() {
    let cases = [
        (OverlaySide::Above, Vec2::new(160.0, 54.0)),
        (OverlaySide::Below, Vec2::new(160.0, 166.0)),
        (OverlaySide::Left, Vec2::new(54.0, 120.0)),
        (OverlaySide::Right, Vec2::new(246.0, 120.0)),
    ];
    for (side, expected) in cases {
        let position = OverlayPosition::new(ANCHOR, side, AxisAlign::Start, 6.0);
        assert_eq!(position.resolve(BODY, BOUNDS), expected, "{side:?}");
    }
}

#[test]
fn overflowing_preferred_side_flips_across_the_anchor() {
    let cases = [
        (
            OverlayPosition::new(
                Rect::new(160.0, 4.0, 80.0, 40.0),
                OverlaySide::Above,
                AxisAlign::Start,
                6.0,
            ),
            Vec2::new(160.0, 50.0),
        ),
        (
            OverlayPosition::new(
                Rect::new(160.0, 256.0, 80.0, 40.0),
                OverlaySide::Below,
                AxisAlign::Start,
                6.0,
            ),
            Vec2::new(160.0, 190.0),
        ),
        (
            OverlayPosition::new(
                Rect::new(4.0, 120.0, 80.0, 40.0),
                OverlaySide::Left,
                AxisAlign::Start,
                6.0,
            ),
            Vec2::new(90.0, 120.0),
        ),
        (
            OverlayPosition::new(
                Rect::new(316.0, 120.0, 80.0, 40.0),
                OverlaySide::Right,
                AxisAlign::Start,
                6.0,
            ),
            Vec2::new(210.0, 120.0),
        ),
    ];
    for (position, expected) in cases {
        assert_eq!(position.resolve(BODY, BOUNDS), expected);
    }
}

#[test]
fn cross_axis_alignment_uses_the_full_anchor_rect() {
    let cases = [
        (AxisAlign::Start, 160.0),
        (AxisAlign::Center, 150.0),
        (AxisAlign::End, 140.0),
    ];
    for (align, expected_x) in cases {
        let position = OverlayPosition::new(ANCHOR, OverlaySide::Below, align, 0.0);
        assert_eq!(
            position.resolve(BODY, BOUNDS),
            Vec2::new(expected_x, 160.0),
            "{align:?}",
        );
    }
}

#[test]
fn impossible_fit_clamps_inside_bounds() {
    let position = OverlayPosition::new(
        Rect::new(390.0, 140.0, 10.0, 20.0),
        OverlaySide::Below,
        AxisAlign::Start,
        6.0,
    );
    assert_eq!(
        position.resolve(Size::new(500.0, 400.0), BOUNDS),
        Vec2::ZERO,
    );
}
