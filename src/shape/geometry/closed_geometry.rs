//! [`ClosedGeometry`]: the closed shape a record covers, a box or a triangle, which places where
//! it is drawn ([`PlacedClosed`]).

use crate::common::hash::Hasher;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::box_geometry::{BoxGeometry, PlacedBox};
use crate::shape::geometry::triangle_geometry::{self, PlacedTriangle, TriangleGeometry};
use std::hash::Hasher as _;

/// The closed shape a paint covers, owner-relative. What an image or an effect shows fits the
/// shape's rect: a box's own, or a triangle's covering rect, as its gradient spans it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ClosedGeometry {
    /// A rect's or a circle's box, and the side of it the paint covers.
    Box(BoxGeometry),
    Triangle(TriangleGeometry),
}

/// A [`ClosedGeometry`] placed in its owner, in the owner's logical frame, which a payload packs
/// through the shape template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlacedClosed {
    Box {
        placed: PlacedBox,
        coverage: Coverage,
    },
    Triangle(PlacedTriangle),
}

impl ClosedGeometry {
    /// The geometry placed in an owner at `owner_rect`: placed owner-local, then moved by the
    /// owner's position, as a box's fill and its cull bound are; `None` for a triangle whose
    /// fitted area paints nothing.
    pub(crate) fn place(self, owner_rect: Rect) -> Option<PlacedClosed> {
        match self {
            Self::Box(geometry) => Some(PlacedClosed::Box {
                placed: geometry.place(owner_rect.size).translated(owner_rect.min),
                coverage: geometry.coverage,
            }),
            Self::Triangle(geometry) => geometry
                .place(owner_rect.size)
                .map(|placed| PlacedClosed::Triangle(placed.translated(owner_rect.min))),
        }
    }

    /// The shape's covering rect in an owner of `owner_size`, owner-local: a box's rect, a
    /// circle's square, a triangle's corner centers grown by its radius; [`Rect::ZERO`] for a
    /// triangle that places nothing.
    pub(crate) fn local_bounds(self, owner_size: Size) -> Rect {
        match self {
            Self::Box(geometry) => geometry.place(owner_size).rect,
            Self::Triangle(geometry) => geometry
                .place(owner_size)
                .map_or(Rect::ZERO, PlacedTriangle::bbox),
        }
    }

    /// Whether the shape paints nothing wherever it is placed: a box's rect of its own of no
    /// size, or a triangle of no area, which no fit gives one. The owner's rect is sized where it
    /// is drawn.
    pub(crate) fn is_paint_empty(self) -> bool {
        match self {
            Self::Box(geometry) => geometry.local_rect.is_some_and(Rect::is_paint_empty),
            Self::Triangle(geometry) => triangle_geometry::area_is_paint_empty(geometry.corners),
        }
    }

    /// Whether the shape covers its inside: a triangle, or a box of that side, the shapes an image
    /// and an app's effect take.
    pub(crate) const fn covers_inside(self) -> bool {
        match self {
            Self::Box(geometry) => matches!(geometry.coverage, Coverage::Inside),
            Self::Triangle(_) => true,
        }
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self {
            Self::Box(geometry) => {
                h.write_u8(0);
                geometry.hash_into(h);
            }
            Self::Triangle(geometry) => {
                h.write_u8(1);
                geometry.hash_into(h);
            }
        }
    }
}

impl NanCheck for ClosedGeometry {
    fn has_nan(&self) -> bool {
        match self {
            Self::Box(geometry) => geometry.has_nan(),
            Self::Triangle(geometry) => geometry.has_nan(),
        }
    }
}

impl PlacedClosed {
    /// A box placed in its owner, covering its inside.
    pub(crate) const fn inside(placed: PlacedBox) -> Self {
        Self::Box {
            placed,
            coverage: Coverage::Inside,
        }
    }

    /// The shape's covering rect: a box's own, a circle's square, or the triangle's box, its
    /// corners' centers grown by its radius, which its gradient, its image and its effect span.
    pub(crate) fn bounds(self) -> Rect {
        match self {
            Self::Box { placed, .. } => placed.rect,
            Self::Triangle(placed) => placed.bbox(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::coverage::Coverage;
    use crate::primitives::geometry::rect::Rect;
    use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline, PlacedBox};
    use crate::shape::geometry::closed_geometry::{ClosedGeometry, PlacedClosed};
    use crate::shape::geometry::point_placement::PointPlacement;
    use crate::shape::geometry::triangle_geometry::{PlacedTriangle, TriangleGeometry};
    use glam::Vec2;

    /// A placed shape keeps the side its geometry covers, and moves by the owner's position: a
    /// rect of its own at (3, 4) in an owner at (100, 200) lies at (103, 204); a circle in the
    /// owner's 8 × 4 rect at (10.5, 20.25) is the 4 × 4 square centered in it, (12.5, 20.25),
    /// rounded by 2; a sharp triangle's corners, its corner centers, move by (100, 200), and one
    /// of no area places nothing.
    #[test]
    fn a_placed_shape_keeps_its_side_and_moves_by_its_owner() {
        let outside = ClosedGeometry::Box(BoxGeometry {
            local_rect: Some(Rect::new(3.0, 4.0, 10.0, 6.0)),
            outline: BoxOutline::Rect(Corners::all(2.0)),
            coverage: Coverage::Outside,
        });
        assert_eq!(
            outside.place(Rect::new(100.0, 200.0, 50.0, 50.0)),
            Some(PlacedClosed::Box {
                placed: PlacedBox {
                    rect: Rect::new(103.0, 204.0, 10.0, 6.0),
                    corners: Corners::all(2.0),
                },
                coverage: Coverage::Outside,
            })
        );
        let circle = ClosedGeometry::Box(BoxGeometry {
            local_rect: None,
            outline: BoxOutline::Circle,
            coverage: Coverage::Inside,
        });
        assert_eq!(
            circle.place(Rect::new(10.5, 20.25, 8.0, 4.0)),
            Some(PlacedClosed::inside(PlacedBox {
                rect: Rect::new(12.5, 20.25, 4.0, 4.0),
                corners: Corners::all(2.0),
            }))
        );
        let corners = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 4.0)];
        let triangle = |corners| {
            ClosedGeometry::Triangle(TriangleGeometry {
                corners,
                radius: 0.0,
                placement: PointPlacement::NATURAL,
            })
        };
        assert_eq!(
            triangle(corners).place(Rect::new(100.0, 200.0, 50.0, 50.0)),
            Some(PlacedClosed::Triangle(PlacedTriangle {
                centers: [
                    Vec2::new(100.0, 200.0),
                    Vec2::new(104.0, 200.0),
                    Vec2::new(100.0, 204.0),
                ],
                radius: 0.0,
            }))
        );
        let flat = [Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)];
        assert_eq!(
            triangle(flat).place(Rect::new(100.0, 200.0, 50.0, 50.0)),
            None
        );
        assert!(!outside.covers_inside());
        assert!(circle.covers_inside());
        assert!(triangle(corners).covers_inside());
    }
}
