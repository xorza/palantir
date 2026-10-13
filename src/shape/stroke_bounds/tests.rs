use crate::primitives::geometry::rect::Rect;
use crate::shape::stroke_bounds::{StrokeJoints, StrokeReach};
use crate::shape::style::{LineCap, LineJoin};
use std::f32::consts;

#[test]
fn stroke_bounds_account_for_cap_and_join_reach_once() {
    #[derive(Debug)]
    struct Case {
        cap: LineCap,
        joints: StrokeJoints,
        expected_pad: f32,
    }

    let cases = [
        Case {
            cap: LineCap::Butt,
            joints: StrokeJoints::Unjoined,
            expected_pad: 2.5,
        },
        Case {
            cap: LineCap::Round,
            joints: StrokeJoints::Joined(LineJoin::Bevel),
            expected_pad: 2.5,
        },
        Case {
            cap: LineCap::Square,
            joints: StrokeJoints::Joined(LineJoin::Round),
            expected_pad: 2.5 * consts::SQRT_2,
        },
        Case {
            cap: LineCap::Butt,
            joints: StrokeJoints::Joined(LineJoin::Miter),
            expected_pad: 10.0,
        },
        // Both factors above 1: the larger wins, `2.5 · max(√2, 4) = 10`,
        // where a product of the two would reach `14.14`.
        Case {
            cap: LineCap::Square,
            joints: StrokeJoints::Joined(LineJoin::Miter),
            expected_pad: 10.0,
        },
    ];
    let centerline = Rect::new(10.0, 20.0, 30.0, 40.0);

    for case in cases {
        let actual = StrokeReach {
            width: 4.0,
            fringe: 0.5,
            cap: case.cap,
            joints: case.joints,
        }
        .bbox(centerline);
        assert_eq!(
            actual,
            Rect::new(
                10.0 - case.expected_pad,
                20.0 - case.expected_pad,
                30.0 + 2.0 * case.expected_pad,
                40.0 + 2.0 * case.expected_pad,
            ),
            "{case:?}",
        );
    }
}

/// A polyline joins at its interior points: two points are one segment.
#[test]
fn a_polyline_joins_only_past_one_segment() {
    for (points, joints) in [
        (0, StrokeJoints::Unjoined),
        (2, StrokeJoints::Unjoined),
        (3, StrokeJoints::Joined(LineJoin::Miter)),
    ] {
        assert_eq!(
            StrokeJoints::polyline(points, LineJoin::Miter),
            joints,
            "{points}"
        );
    }
}

/// A negative width or fringe adds nothing rather than shrinking the
/// bound: `(max(-2, 0) + 0.5) · 1 = 0.5`, and `(4 / 2 + max(-1, 0)) · 1 = 2`.
#[test]
fn negative_width_and_fringe_clamp_to_zero() {
    let centerline = Rect::new(10.0, 20.0, 30.0, 40.0);
    for (width, fringe, pad) in [(-4.0, 0.5, 0.5), (4.0, -1.0, 2.0)] {
        assert_eq!(
            StrokeReach {
                width,
                fringe,
                cap: LineCap::Butt,
                joints: StrokeJoints::Unjoined,
            }
            .bbox(centerline),
            Rect::new(10.0 - pad, 20.0 - pad, 30.0 + 2.0 * pad, 40.0 + 2.0 * pad),
            "width {width}, fringe {fringe}",
        );
    }
}
