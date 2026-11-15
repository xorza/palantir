//! One raster key's entry: the raster the pool holds for it or none, and the placement a draw reads
//! it at.

use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::gpu::raster::raster_atlas::content_of;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use glam::{I16Vec2, IVec2, UVec2};

/// A raster the pool holds, and where it sits now, as the pool places it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotPlacement {
    pub(crate) raster: PlacedRaster,
    pub(crate) placement: Placement,
}

impl SlotPlacement {
    /// What the raster holds, by the list its handle names: also the sampling mode.
    pub(crate) const fn content(self) -> ContentType {
        content_of(self.raster.handle.format)
    }

    /// The instance drawing this raster at `pen` plus the bearing.
    pub(crate) fn quad(self, pen: IVec2, color: RgbaF16) -> RasterQuad {
        // Bearing `y` is up; screen `y` is down.
        let bearing = self.raster.bearing;
        let pos = IVec2::new(pen.x + i32::from(bearing.x), pen.y - i32::from(bearing.y));
        RasterQuad::placed(pos, self.placement, self.content(), color)
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
}

impl AtlasSlot {
    /// The raster the entry draws, `None` for one that draws nothing.
    #[inline]
    pub(crate) const fn raster(&self) -> Option<PlacedRaster> {
        match *self {
            Self::Placed(raster) => Some(raster),
            Self::Unplaced { .. } => None,
        }
    }
}
