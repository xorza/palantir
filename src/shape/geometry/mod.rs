//! The geometry a shape record keeps, owner-relative, and what it places once the owner's size is
//! known: a box, a triangle, a stroke's path, a point shape's placement and a stroke's trace.

pub(crate) mod arc_geometry;
pub(crate) mod bezier_geometry;
pub(crate) mod box_geometry;
pub(crate) mod closed_geometry;
pub(crate) mod curve_basis;
pub(crate) mod placed_stroke;
pub(crate) mod point_placement;
pub(crate) mod stroke_geometry;
pub(crate) mod triangle_geometry;

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use glam::Vec2;

/// The rect a shape lays out in, owner-local: its own `local_rect`, or its owner's whole rect, of
/// `owner_size`.
pub(crate) fn layout_rect(local_rect: Option<Rect>, owner_size: Size) -> Rect {
    local_rect.unwrap_or(Rect {
        min: Vec2::ZERO,
        size: owner_size,
    })
}
