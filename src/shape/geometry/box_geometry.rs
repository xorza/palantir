//! [`BoxGeometry`]: where a rect's or a circle's box lies in its owner, and the box it places.

use crate::common::hash::Hasher;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use glam::Vec2;
use std::hash::{Hash as _, Hasher as _};

/// Where a box lies: `local_rect = None` lays it out in the owner's arranged rect, `Some(r)` in
/// owner-relative coords, in the slot it was pushed in, under the owner's clip but outside its pan
/// transform; its outline in that rect, and the side of it it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxGeometry {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) outline: BoxOutline,
    pub(crate) coverage: Coverage,
}

impl BoxGeometry {
    /// The box in an owner of `owner_size`, owner-local.
    pub(crate) fn place(self, owner_size: Size) -> PlacedBox {
        self.outline.place(self.local_rect.unwrap_or(Rect {
            min: Vec2::ZERO,
            size: owner_size,
        }))
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        let Self {
            local_rect,
            outline,
            coverage,
        } = self;
        h.write_u8(coverage as u8);
        local_rect.hash_visual(h);
        outline.hash_into(h);
    }
}

impl NanCheck for BoxGeometry {
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.outline.has_nan()
    }
}

/// The boundary of a box in the rect it lays out in: a rounded rect, or the largest circle
/// centered in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BoxOutline {
    Rect(Corners),
    Circle,
}

/// A box placed: the rect its quad covers and that rect's corner radii.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedBox {
    pub(crate) rect: Rect,
    pub(crate) corners: Corners,
}

impl PlacedBox {
    /// The box moved by `by`, as an owner's position places an owner-local box.
    pub(crate) fn translated(self, by: Vec2) -> Self {
        Self {
            rect: Rect {
                min: self.rect.min + by,
                size: self.rect.size,
            },
            corners: self.corners,
        }
    }
}

impl BoxOutline {
    /// Feed the outline into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        match self {
            Self::Rect(corners) => {
                h.write_u8(0);
                corners.hash(h);
            }
            Self::Circle => h.write_u8(1),
        }
    }

    /// The box this outline makes in `rect`: the rect itself with its corners, or the square
    /// centered in it, its side the shorter of the two, rounded by half that side.
    pub(crate) fn place(self, rect: Rect) -> PlacedBox {
        match self {
            Self::Rect(corners) => PlacedBox { rect, corners },
            Self::Circle => {
                let side = rect.size.w.min(rect.size.h);
                let square = Rect {
                    min: rect.min + 0.5 * Vec2::new(rect.size.w - side, rect.size.h - side),
                    size: Size::new(side, side),
                };
                PlacedBox {
                    rect: square,
                    corners: Corners::saturating([0.5 * side; 4]),
                }
            }
        }
    }
}

impl NanCheck for BoxOutline {
    fn has_nan(&self) -> bool {
        match self {
            Self::Rect(corners) => corners.has_nan(),
            Self::Circle => false,
        }
    }
}
