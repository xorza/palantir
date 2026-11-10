//! The quad-drawn shapes a record lowers to.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::shape::geometry::box_geometry::BoxGeometry;
use crate::shape::geometry::triangle_geometry::{PlacedTriangle, TriangleGeometry};
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_shadow::LoweredShadow;

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
    /// Filled or bordered rounded triangle, its corners placed against the owner where it is
    /// drawn ([`TriangleGeometry::place`]), with its material, which makes it a backdrop as a
    /// box's does.
    Triangle {
        geometry: TriangleGeometry,
        fill: LoweredBrush,
        border: LoweredBorder,
        material: Option<LoweredMaterial>,
    },
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
                .map_or(Rect::ZERO, PlacedTriangle::bbox),
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
                material,
            } => geometry.has_nan() || fill.has_nan() || border.has_nan() || material.has_nan(),
        }
    }
}
