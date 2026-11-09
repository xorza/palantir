use super::*;
use crate::primitives::geometry::corners::Corners;
use crate::shape::paint::quad_shape::PlacedBox;
use std::f32::consts::PI;

const OWNER: Size = Size::new(40.0, 20.0);

fn arc(local_rect: Option<Rect>, sweep: f32) -> ArcGeometry {
    ArcGeometry {
        local_rect,
        start_angle: 0.0,
        sweep,
    }
}

/// The circle is the largest one centered in the rect: the owner's 40×20 gives center (20, 10)
/// and radius 20 / 2 = 10; a rect of its own, 10×30 at (10, 0), gives (15, 15) and 10 / 2 = 5.
#[test]
fn the_circle_is_the_largest_centered_in_the_rect() {
    assert_eq!(
        arc(None, PI).place(OWNER, LineCap::Butt, 2.0),
        PlacedStroke::Curve(CurveBasis::Arc(CircularArc {
            center: Vec2::new(20.0, 10.0),
            radius: 10.0,
            start_angle: 0.0,
            sweep: PI,
        })),
    );
    assert_eq!(
        arc(Some(Rect::new(10.0, 0.0, 10.0, 30.0)), PI).place(OWNER, LineCap::Butt, 2.0),
        PlacedStroke::Curve(CurveBasis::Arc(CircularArc {
            center: Vec2::new(15.0, 15.0),
            radius: 5.0,
            start_angle: 0.0,
            sweep: PI,
        })),
    );
}

/// No sweep puts the dot at the start, center + radius × (cos 0, sin 0) = (30, 10), the width 4
/// across: (28, 8) to (32, 12). The cap picks its outline, and a butt cap has no extent.
#[test]
fn an_arc_of_no_sweep_paints_its_cap() {
    let dot = |cap| arc(None, 0.0).place(OWNER, cap, 4.0);
    assert_eq!(
        dot(LineCap::Round),
        PlacedStroke::Dot(PlacedBox {
            rect: Rect::new(28.0, 8.0, 4.0, 4.0),
            corners: Corners::saturating([2.0; 4]),
        }),
    );
    assert_eq!(
        dot(LineCap::Square),
        PlacedStroke::Dot(PlacedBox {
            rect: Rect::new(28.0, 8.0, 4.0, 4.0),
            corners: Corners::ZERO,
        }),
    );
    assert_eq!(dot(LineCap::Butt), PlacedStroke::Nothing);
}

/// A rect with no height has no radius, so the dot sits on its center, (0 + 40 / 2, 0) = (20, 0),
/// whatever the sweep.
#[test]
fn an_arc_of_no_radius_paints_its_cap_at_the_center() {
    assert_eq!(
        arc(Some(Rect::new(0.0, 0.0, 40.0, 0.0)), PI).place(OWNER, LineCap::Square, 2.0),
        PlacedStroke::Dot(PlacedBox {
            rect: Rect::new(19.0, -1.0, 2.0, 2.0),
            corners: Corners::ZERO,
        }),
    );
}
