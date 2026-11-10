//! [`ShapeOutline`]: the closed shape a record covers, which places where it is drawn
//! ([`PlacedOutline`]).

use crate::common::hash::Hasher;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::triangle_geometry::{self, TriangleGeometry};
use glam::Vec2;
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

/// A [`ShapeOutline`] placed in logical px, which a payload's quad packs through the shape
/// template: a rounded rect, or a triangle placed as a triangle quad is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlacedOutline {
    /// A rect and its corner radii.
    Rect { rect: Rect, corners: Corners },
    /// A triangle's corner centers, dilated by `radius`.
    Triangle { points: [Vec2; 3], radius: f32 },
}

impl ShapeOutline {
    /// The outline placed in an owner at `owner_rect`, as [`Rect::resolve_local`] places a rect;
    /// `None` for a triangle whose fitted area paints nothing.
    pub(crate) fn place(self, owner_rect: Rect) -> Option<PlacedOutline> {
        match self {
            Self::Rect {
                local_rect,
                corners,
            } => Some(PlacedOutline::Rect {
                rect: owner_rect.resolve_local(local_rect),
                corners,
            }),
            Self::Triangle(geometry) => {
                let placed = geometry.place(owner_rect.size)?;
                Some(PlacedOutline::Triangle {
                    points: placed.centers.map(|q| owner_rect.min + q),
                    radius: placed.radius,
                })
            }
        }
    }

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

impl PlacedOutline {
    /// The shape's covering rect: a rect's own, or the triangle's bounds, its corners' centers
    /// grown by its radius, which its gradient, its image and its effect span.
    pub(crate) fn bounds(self) -> Rect {
        match self {
            Self::Rect { rect, .. } => rect,
            Self::Triangle { points, radius } => Aabb::of(&points).inflated(radius),
        }
    }
}
