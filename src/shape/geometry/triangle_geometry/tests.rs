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
        }),
    );
    assert_eq!(
        geometry
            .place(Size::new(40.0, 20.0))
            .map(PlacedTriangle::bbox),
        Some(Rect::new(10.0, 0.0, 20.0, 20.0)),
    );
    assert_eq!(geometry.place(Size::new(0.0, 20.0)), None);
}

/// A translation moves every center by the offset and keeps the radius, and the box follows: the
/// centers (10, 0), (30, 0), (10, 20) of radius 2 moved by (5, −3) are (15, −3), (35, −3),
/// (15, 17), boxed (13, −5) to (37, 19).
#[test]
fn a_translation_moves_the_centers_and_their_box() {
    let placed = PlacedTriangle {
        centers: [
            Vec2::new(10.0, 0.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(10.0, 20.0),
        ],
        radius: 2.0,
    }
    .translated(Vec2::new(5.0, -3.0));
    assert_eq!(
        placed,
        PlacedTriangle {
            centers: [
                Vec2::new(15.0, -3.0),
                Vec2::new(35.0, -3.0),
                Vec2::new(15.0, 17.0),
            ],
            radius: 2.0,
        }
    );
    assert_eq!(placed.bbox(), Rect::new(13.0, -5.0, 24.0, 24.0));
}
