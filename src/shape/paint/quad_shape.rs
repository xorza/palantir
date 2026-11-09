//! The quad-drawn shapes a record lowers to.

use crate::common::hash::Hasher;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::triangle_geometry::TriangleGeometry;
use glam::Vec2;
use std::hash::{Hash as _, Hasher as _};

/// Which shape a quad-tier draw paints: a box or a rounded triangle. Both lower to a single
/// `Quad` on one pipeline, selected by its `fill_kind` lane, in [`crate::shape::lower`]. The
/// composer's fast paths are gated on `fill_kind == FillKind::SOLID`, which only [`Self::Box`]
/// carries. Both are owner-local.
#[derive(Clone, Copy, Debug)]
pub(crate) enum QuadShape {
    /// A rect's or a circle's paint: a filled or bordered box, placed against the owner where it
    /// is drawn ([`BoxGeometry::place`]), with its shadow and its material. A material that
    /// captures makes it a backdrop over the pixels painted before it.
    Box {
        geometry: BoxGeometry,
        fill: LoweredBrush,
        border: LoweredBorder,
        shadow: LoweredShadow,
        material: Option<LoweredMaterial>,
    },
    /// Filled or bordered rounded triangle as an analytic SDF, solid fills only, its corners
    /// placed against the owner where it is drawn ([`TriangleGeometry::place`]).
    Triangle {
        geometry: TriangleGeometry,
        /// Solid linear-RGB fill, straight alpha.
        fill: RgbaF16,
        border: LoweredBorder,
    },
}

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
        match local_rect {
            None => h.write_u8(0),
            Some(rect) => {
                h.write_u8(1);
                rect.hash_visual(h);
            }
        }
        match outline {
            BoxOutline::Rect(corners) => {
                h.write_u8(0);
                corners.hash(h);
            }
            BoxOutline::Circle => h.write_u8(1),
        }
    }
}

impl NanCheck for BoxGeometry {
    fn has_nan(&self) -> bool {
        let corners_nan = match self.outline {
            BoxOutline::Rect(corners) => corners.has_nan(),
            BoxOutline::Circle => false,
        };
        self.local_rect.has_nan() || corners_nan
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

impl BoxOutline {
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

impl QuadShape {
    /// Owner-local paint bbox, the basis for the screen-space paint bound.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            Self::Box {
                geometry, shadow, ..
            } => {
                let rect = geometry.place(owner_size).rect;
                if shadow.is_noop() {
                    rect
                } else {
                    rect.union(shadow.paint_rect_local(Some(rect), owner_size))
                }
            }
            Self::Triangle { geometry, .. } => geometry
                .place(owner_size)
                .map_or(Rect::ZERO, |placed| placed.bbox),
        }
    }
}

impl NanCheck for QuadShape {
    fn has_nan(&self) -> bool {
        match self {
            Self::Box {
                geometry,
                fill,
                border,
                shadow,
                material,
            } => {
                geometry.has_nan()
                    || fill.has_nan()
                    || border.has_nan()
                    || shadow.has_nan()
                    || material.has_nan()
            }
            Self::Triangle {
                geometry,
                fill,
                border,
            } => geometry.has_nan() || fill.has_nan() || border.has_nan(),
        }
    }
}
