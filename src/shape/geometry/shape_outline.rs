//! [`ShapeOutline`]: the closed shape a record covers, which places where it is drawn.

use crate::common::hash::Hasher;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::triangle_geometry::{self, TriangleGeometry};
use std::hash::{Hash as _, Hasher as _};

/// The closed shape an image or an effect fills and is clipped to, owner-relative. What it shows
/// fits the shape's rect: a rect's own, or a triangle's covering rect, as its gradient spans it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ShapeOutline {
    /// The owner's rect (`None`) or one of its own, owner-relative, and its corner radii.
    Rect {
        local_rect: Option<Rect>,
        corners: Corners,
    },
    Triangle(TriangleGeometry),
}

impl ShapeOutline {
    /// Whether the shape paints nothing wherever it is placed: a rect of its own of no size, or a
    /// triangle of no area, which no fit gives one. The owner's rect is sized where it is drawn.
    pub(crate) fn is_paint_empty(self) -> bool {
        match self {
            Self::Rect { local_rect, .. } => local_rect.is_some_and(Rect::is_paint_empty),
            Self::Triangle(geometry) => triangle_geometry::area_is_paint_empty(geometry.corners),
        }
    }

    /// Feed the outline into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self {
            Self::Rect {
                local_rect,
                corners,
            } => {
                h.write_u8(0);
                match local_rect {
                    None => h.write_u8(0),
                    Some(rect) => {
                        h.write_u8(1);
                        rect.hash_visual(h);
                    }
                }
                corners.hash(h);
            }
            Self::Triangle(geometry) => {
                h.write_u8(1);
                geometry.hash_into(h);
            }
        }
    }
}

impl NanCheck for ShapeOutline {
    fn has_nan(&self) -> bool {
        match self {
            Self::Rect {
                local_rect,
                corners,
            } => local_rect.has_nan() || corners.has_nan(),
            Self::Triangle(geometry) => geometry.has_nan(),
        }
    }
}
