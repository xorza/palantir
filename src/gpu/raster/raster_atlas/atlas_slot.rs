//! One raster key's entry: the raster the pool holds for it or none, and the placement a draw reads
//! it at.

use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::gpu::raster::raster_atlas::content_of;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

/// Where a packed raster sits now, as the pool places it, with its handle and bearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotPlacement {
    pub(crate) handle: AtlasHandle,
    pub(crate) page: u8,
    /// Top-left texel on its page.
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
    /// Pen position to the raster's top-left; `y` up.
    pub(crate) bearing: I16Vec2,
}

impl SlotPlacement {
    /// The raster at `placement`, of `raster`'s bearing.
    pub(crate) const fn of(placement: Placement, raster: PlacedRaster) -> Self {
        Self {
            handle: raster.handle,
            page: placement.page,
            origin: placement.origin,
            size: placement.size,
            bearing: raster.bearing,
        }
    }

    /// What the raster holds, by the list its handle names: also the sampling mode.
    pub(crate) const fn content(self) -> ContentType {
        content_of(self.handle.format)
    }

    /// The instance drawing this raster at `pen` plus the bearing.
    pub(crate) fn quad(self, pen: IVec2, color: RgbaF16) -> RasterQuad {
        // Bearing `y` is up; screen `y` is down.
        let pos = IVec2::new(
            pen.x + i32::from(self.bearing.x),
            pen.y - i32::from(self.bearing.y),
        );
        let placement = Placement {
            page: self.page,
            origin: self.origin,
            size: self.size,
        };
        RasterQuad::placed(pos, placement, self.content(), color)
    }

    /// [`Self::quad`] resampled to `size` physical px, for an icon outside the exact band.
    pub(crate) fn quad_sized(self, pen: IVec2, size: UVec2, color: RgbaF16) -> RasterQuad {
        RasterQuad {
            size,
            ..self.quad(pen, color)
        }
    }
}

/// A raster the pool holds: its handle, whose list says what it holds, and its bearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlacedRaster {
    pub(crate) handle: AtlasHandle,
    pub(crate) bearing: I16Vec2,
}

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s slab; narrow and `Copy` for the per-glyph
/// hit path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtlasSlot {
    /// A raster the pool holds, which it may evict.
    Placed(PlacedRaster),
    /// A live entry that draws nothing (whitespace, no pixels, past a page), which expires an
    /// expiry window after `last_use`, the pool's display frame it was last looked up in.
    Unplaced { last_use: u64 },
    /// On the free list, named by no cache key.
    Free,
}

impl AtlasSlot {
    /// The raster the entry draws, `None` for one that draws nothing or is free.
    #[inline]
    pub(crate) const fn raster(&self) -> Option<PlacedRaster> {
        match *self {
            Self::Placed(raster) => Some(raster),
            Self::Unplaced { .. } | Self::Free => None,
        }
    }
}
