//! [`ClosedGeometry`]: the closed shape an image or an effect record covers, a box or a
//! triangle, which places where it is drawn ([`PlacedClosed`]).

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::box_geometry::{BoxOutline, PlacedBox};
use crate::shape::geometry::triangle_geometry::{self, PlacedTriangle, TriangleGeometry};
use std::hash::Hasher as _;

/// The closed shape an image or an effect fills and is clipped to, owner-relative. What it shows
/// fits the shape's rect: a box's own, or a triangle's covering rect, as its gradient spans it.
/// A box covers its inside alone, as the image and effect stages clip to no other side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ClosedGeometry {
    /// A rect's or a circle's box, laid out in the owner's rect (`None`) or one of its own,
    /// owner-relative.
    Box {
        local_rect: Option<Rect>,
        outline: BoxOutline,
    },
    Triangle(TriangleGeometry),
}

/// A [`ClosedGeometry`] placed in its owner, in the owner's logical frame, which a payload's
/// quad packs through the shape template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlacedClosed {
    Box(PlacedBox),
    Triangle(PlacedTriangle),
}

impl ClosedGeometry {
    /// The geometry placed in an owner at `owner_rect`, as [`Rect::resolve_local`] places a rect;
    /// `None` for a triangle whose fitted area paints nothing.
    pub(crate) fn place(self, owner_rect: Rect) -> Option<PlacedClosed> {
        match self {
            Self::Box {
                local_rect,
                outline,
            } => Some(PlacedClosed::Box(
                outline.place(owner_rect.resolve_local(local_rect)),
            )),
            Self::Triangle(geometry) => geometry
                .place(owner_rect.size)
                .map(|placed| PlacedClosed::Triangle(placed.translated(owner_rect.min))),
        }
    }

    /// Whether the shape paints nothing wherever it is placed: a box's rect of its own of no
    /// size, or a triangle of no area, which no fit gives one. The owner's rect is sized where it
    /// is drawn.
    pub(crate) fn is_paint_empty(self) -> bool {
        match self {
            Self::Box { local_rect, .. } => local_rect.is_some_and(Rect::is_paint_empty),
            Self::Triangle(geometry) => triangle_geometry::area_is_paint_empty(geometry.corners),
        }
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self {
            Self::Box {
                local_rect,
                outline,
            } => {
                h.write_u8(0);
                local_rect.hash_visual(h);
                outline.hash_into(h);
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
            Self::Box {
                local_rect,
                outline,
            } => local_rect.has_nan() || outline.has_nan(),
            Self::Triangle(geometry) => geometry.has_nan(),
        }
    }
}

impl PlacedClosed {
    /// The shape's covering rect: a box's own, a circle's square, or the triangle's box, its
    /// corners' centers grown by its radius, which its gradient, its image and its effect span.
    pub(crate) fn bounds(self) -> Rect {
        match self {
            Self::Box(placed) => placed.rect,
            Self::Triangle(placed) => placed.bbox(),
        }
    }
}
