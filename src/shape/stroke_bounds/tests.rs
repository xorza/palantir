use crate::primitives::rect::Rect;
use crate::shape::stroke_bounds;
use crate::shape::style::{LineCap, LineJoin};

#[test]
fn stroke_bounds_account_for_cap_and_join_reach_once() {
    #[derive(Debug)]
    struct Case {
        cap: LineCap,
        join: Option<LineJoin>,
        expected_pad: f32,
    }

    let cases = [
        Case {
            cap: LineCap::Butt,
            join: None,
            expected_pad: 2.5,
        },
        Case {
            cap: LineCap::Round,
            join: Some(LineJoin::Bevel),
            expected_pad: 2.5,
        },
        Case {
            cap: LineCap::Square,
            join: Some(LineJoin::Round),
            expected_pad: 2.5 * std::f32::consts::SQRT_2,
        },
        Case {
            cap: LineCap::Butt,
            join: Some(LineJoin::Miter),
            expected_pad: 10.0,
        },
    ];
    let centerline = Rect::new(10.0, 20.0, 30.0, 40.0);

    for case in cases {
        let actual = stroke_bounds::bbox(centerline, 4.0, 0.5, case.cap, case.join);
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
