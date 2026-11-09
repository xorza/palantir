use super::*;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::shape::paint::quad_shape::PlacedBox;
use glam::Vec2;

/// The line (0, 0)–(9, 3) as a cubic on its thirds.
fn line() -> CubicBezier {
    CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::new(3.0, 1.0),
        p2: Vec2::new(6.0, 2.0),
        p3: Vec2::new(9.0, 3.0),
    }
}

fn place(
    bezier: CubicBezier,
    placement: PointPlacement,
    owner: Size,
    cap: LineCap,
) -> PlacedStroke {
    BezierGeometry { bezier, placement }.place(owner, cap, 2.0)
}

/// `Natural` in a rect at (10, 20) moves every point by it; `Fill` in an 18 × 6 owner scales the
/// 9 × 3 extent by 2 on each axis.
#[test]
fn the_frame_maps_every_control_point() {
    let at = PointPlacement {
        local_rect: Some(Rect::new(10.0, 20.0, 1.0, 1.0)),
        fit: Fit::Natural,
    };
    assert_eq!(
        place(line(), at, Size::ZERO, LineCap::Butt),
        PlacedStroke::Curve(CurveBasis::Cubic(CubicBezier {
            p0: Vec2::new(10.0, 20.0),
            p1: Vec2::new(13.0, 21.0),
            p2: Vec2::new(16.0, 22.0),
            p3: Vec2::new(19.0, 23.0),
        })),
    );
    let fill = PointPlacement {
        local_rect: None,
        fit: Fit::Fill,
    };
    assert_eq!(
        place(line(), fill, Size::new(18.0, 6.0), LineCap::Butt),
        PlacedStroke::Curve(CurveBasis::Cubic(CubicBezier {
            p0: Vec2::ZERO,
            p1: Vec2::new(6.0, 2.0),
            p2: Vec2::new(12.0, 4.0),
            p3: Vec2::new(18.0, 6.0),
        })),
    );
}

/// A fit that collapses the curve paints its cap: the line (0, 0)–(4, 0) filled into an owner
/// 0 wide scales x by 0 / 4 = 0 and centers its flat y on 10 / 2 = 5, so a round cap 2 wide
/// paints the disc (−1, 4)–(1, 6). A butt cap has no extent.
#[test]
fn a_collapsed_curve_paints_its_cap() {
    let flat = CubicBezier {
        p0: Vec2::ZERO,
        p1: Vec2::new(1.0, 0.0),
        p2: Vec2::new(3.0, 0.0),
        p3: Vec2::new(4.0, 0.0),
    };
    let fill = PointPlacement {
        local_rect: None,
        fit: Fit::Fill,
    };
    let owner = Size::new(0.0, 10.0);
    assert_eq!(
        place(flat, fill, owner, LineCap::Round),
        PlacedStroke::Dot(PlacedBox {
            rect: Rect::new(-1.0, 4.0, 2.0, 2.0),
            corners: Corners::saturating([1.0; 4]),
        }),
    );
    assert_eq!(
        place(flat, fill, owner, LineCap::Butt),
        PlacedStroke::Nothing
    );
}
