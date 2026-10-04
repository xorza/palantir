use crate::input::scroll_targets::{ScrollTargets, WheelDelivery};
use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;

/// Every split the routing makes, against deliveries written out by
/// hand: the axes apart, one row for both, the sideways move and the
/// cases that must not make it.
#[test]
fn route_splits_by_axis_and_moves_only_a_pure_vertical_turn() {
    type Case = (
        &'static str,
        ScrollTargets,
        [f32; 2],
        [f32; 2],
        [Option<WheelDelivery>; 2],
    );
    let row = WidgetId::from_hash("row");
    let page = WidgetId::from_hash("page");
    let at = |target, pixels: [f32; 2], lines: [f32; 2]| {
        Some(WheelDelivery {
            target,
            pixels: Vec2::from(pixels),
            lines: Vec2::from(lines),
        })
    };
    let row_x = ScrollTargets {
        x: Some(row),
        y: None,
    };
    let split = ScrollTargets {
        x: Some(row),
        y: Some(page),
    };
    let cases: [Case; 8] = [
        (
            "diagonal pixels, two rows",
            split,
            [3.0, 4.0],
            [0.0, 0.0],
            [
                at(row, [3.0, 0.0], [0.0, 0.0]),
                at(page, [0.0, 4.0], [0.0, 0.0]),
            ],
        ),
        (
            "a vertical notch skips the row that pans only x",
            split,
            [0.0, 0.0],
            [0.0, 2.0],
            [None, at(page, [0.0, 0.0], [0.0, 2.0])],
        ),
        (
            "one row for both axes takes the halves that sum to the event",
            ScrollTargets::both(row),
            [3.0, 4.0],
            [1.0, 2.0],
            [
                at(row, [3.0, 0.0], [1.0, 0.0]),
                at(row, [0.0, 4.0], [0.0, 2.0]),
            ],
        ),
        (
            "a pure vertical turn with no y row pans the x row",
            row_x,
            [0.0, 5.0],
            [0.0, 2.0],
            [at(row, [5.0, 0.0], [2.0, 0.0]), None],
        ),
        (
            "a horizontal turn reaches the x row as it came",
            row_x,
            [5.0, 0.0],
            [0.0, 0.0],
            [at(row, [5.0, 0.0], [0.0, 0.0]), None],
        ),
        (
            "a diagonal swipe keeps its y apart, and nothing takes it",
            row_x,
            [1.0, 5.0],
            [0.0, 0.0],
            [at(row, [1.0, 0.0], [0.0, 0.0]), None],
        ),
        (
            "a horizontal turn does not move to a y-only row",
            ScrollTargets {
                x: None,
                y: Some(page),
            },
            [5.0, 0.0],
            [0.0, 0.0],
            [None, None],
        ),
        (
            "an empty event delivers nothing",
            split,
            [0.0, 0.0],
            [0.0, 0.0],
            [None, None],
        ),
    ];
    for (label, targets, pixels, lines, expected) in cases {
        assert_eq!(
            targets.route(Vec2::from(pixels), Vec2::from(lines)),
            expected,
            "{label}",
        );
    }
    assert!(row_x.any());
    assert!(!ScrollTargets::default().any());
}
