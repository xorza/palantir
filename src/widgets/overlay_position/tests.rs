use super::OverlayPosition;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

#[test]
fn around_flips_each_overflowing_axis_then_clamps() {
    let bounds = Rect::new(0.0, 0.0, 400.0, 400.0);
    let size = Size::new(160.0, 120.0);

    let flipped = OverlayPosition::around(Vec2::new(395.0, 395.0))
        .resolve(Some(size), bounds)
        .anchor;
    assert_eq!(flipped, Vec2::new(235.0, 275.0));

    let unchanged = OverlayPosition::around(Vec2::new(50.0, 50.0))
        .resolve(Some(size), bounds)
        .anchor;
    assert_eq!(unchanged, Vec2::new(50.0, 50.0));

    let oversized = OverlayPosition::around(Vec2::new(50.0, 200.0))
        .resolve(Some(Size::new(50.0, 500.0)), bounds)
        .anchor;
    assert_eq!(oversized, Vec2::new(50.0, 0.0));
}

#[test]
fn unmeasured_position_uses_preferred_anchor_and_requests_measurement() {
    let bounds = Rect::new(20.0, 30.0, 400.0, 300.0);
    let resolved = OverlayPosition::around(Vec2::new(415.0, 325.0)).resolve(None, bounds);

    assert_eq!(resolved.anchor, Vec2::new(415.0, 325.0));
    assert_eq!(resolved.measure_cap, bounds.size);
    assert!(resolved.needs_measure);
}

#[test]
fn below_prefers_trigger_bottom_and_flips_above() {
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let bubble = Size::new(120.0, 32.0);

    let upper_trigger = Rect::new(50.0, 50.0, 80.0, 24.0);
    let below = OverlayPosition::below(upper_trigger, 6.0)
        .resolve(Some(bubble), bounds)
        .anchor;
    assert_eq!(below, Vec2::new(50.0, 80.0));

    let lower_trigger = Rect::new(50.0, 270.0, 80.0, 24.0);
    let above = OverlayPosition::below(lower_trigger, 6.0)
        .resolve(Some(bubble), bounds)
        .anchor;
    assert_eq!(above, Vec2::new(50.0, 232.0));
}

#[test]
fn below_clamps_to_bounds_when_preferred_and_fallback_do_not_fit() {
    let bounds = Rect::new(10.0, 20.0, 400.0, 300.0);

    let right_trigger = Rect::new(360.0, 70.0, 40.0, 24.0);
    let right = OverlayPosition::below(right_trigger, 6.0)
        .resolve(Some(Size::new(120.0, 32.0)), bounds)
        .anchor;
    assert_eq!(right, Vec2::new(290.0, 100.0));

    let middle_trigger = Rect::new(50.0, 160.0, 80.0, 20.0);
    let tall = OverlayPosition::below(middle_trigger, 6.0)
        .resolve(Some(Size::new(120.0, 200.0)), bounds)
        .anchor;
    assert_eq!(tall, Vec2::new(50.0, 120.0));
}
