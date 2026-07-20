use crate::primitives::rect::Rect;
use crate::shape::style::{LineCap, LineJoin};

/// Half-width of the antialiasing fringe every stroke adds beyond its core
/// half-width, in physical pixels. The curve shader specializes the same value.
pub(crate) const HALF_FRINGE: f32 = 0.5;

/// SVG-convention miter limit shared by CPU bounds, composition, and the
/// specialized curve shader.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// Conservative paint bound for a centerline AABB. `width` and `fringe` use
/// the same coordinate space as `centerline`.
pub(crate) fn stroked_bbox(
    centerline: Rect,
    width: f32,
    fringe: f32,
    cap: LineCap,
    join: Option<LineJoin>,
) -> Rect {
    let cap_factor = match cap {
        LineCap::Square => std::f32::consts::SQRT_2,
        LineCap::Butt | LineCap::Round => 1.0,
    };
    let join_factor = match join {
        Some(LineJoin::Miter) => MITER_LIMIT,
        Some(LineJoin::Bevel | LineJoin::Round) | None => 1.0,
    };
    let pad = ((width * 0.5).max(0.0) + fringe.max(0.0)) * cap_factor.max(join_factor);
    centerline.inflated(pad)
}

#[cfg(test)]
mod tests {
    use crate::primitives::rect::Rect;
    use crate::shape::stroke_bounds::stroked_bbox;
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
            let actual = stroked_bbox(centerline, 4.0, 0.5, case.cap, case.join);
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
}
