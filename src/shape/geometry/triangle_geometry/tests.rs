use super::*;
use crate::primitives::geometry::fit::Fit;

/// The corners (0, 0), (10, 0), (0, 10) under `Fit::Contain` in a 40 × 20 owner scale by
/// min(40, 20) / 10 = 2 about the centers, offset `(20, 10) − (5, 5) · 2 = (10, 0)`: (10, 0),
/// (30, 0), (10, 20). Sharp, the centers are the corners and the box is their AABB. A rect of no
/// size collapses the triangle, which then paints nothing.
#[test]
fn the_corners_fit_the_owner_where_placed() {
    let geometry = TriangleGeometry {
        corners: [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
        radius: 0.0,
        placement: PointPlacement {
            local_rect: None,
            fit: Fit::Contain,
        },
    };
    assert_eq!(
        geometry.place(Size::new(40.0, 20.0)),
        Some(PlacedTriangle {
            centers: [
                Vec2::new(10.0, 0.0),
                Vec2::new(30.0, 0.0),
                Vec2::new(10.0, 20.0),
            ],
            radius: 0.0,
            bbox: Rect::new(10.0, 0.0, 20.0, 20.0),
        }),
    );
    assert_eq!(geometry.place(Size::new(0.0, 20.0)), None);
}
