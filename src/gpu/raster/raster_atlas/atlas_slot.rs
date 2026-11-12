//! One resident raster's handle into the pool, its bearing and content, and the placement a draw
//! reads it at.

use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

/// Where a packed raster sits now, as the pool places it, with its handle, bearing and content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotPlacement {
    pub(crate) handle: AtlasHandle,
    pub(crate) page: u8,
    /// Top-left texel on its page.
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
    /// Pen position to the raster's top-left; `y` up.
    pub(crate) bearing: I16Vec2,
    /// Which list holds the rectangle, also the sampling mode.
    pub(crate) content: ContentType,
}

impl SlotPlacement {
    /// The raster at `placement`, of `raster`'s bearing and content.
    pub(crate) const fn of(placement: Placement, raster: PlacedRaster) -> Self {
        Self {
            handle: raster.handle,
            page: placement.page,
            origin: placement.origin,
            size: placement.size,
            bearing: raster.bearing,
            content: raster.content,
        }
    }

    /// The instance drawing this raster at `pen` plus the bearing.
    pub(crate) fn quad(self, pen: IVec2, color: RgbaF16) -> RasterQuad {
        RasterQuad {
            // Bearing `y` is up; screen `y` is down.
            pos: IVec2::new(
                pen.x + i32::from(self.bearing.x),
                pen.y - i32::from(self.bearing.y),
            ),
            dim: self.size,
            size: self.size.as_uvec2(),
            uv_and_kind: RasterQuad::pack_uv(self.origin.x, self.origin.y, self.page, self.content),
            color,
            clip: ClipChain::NONE,
        }
    }

    /// [`Self::quad`] resampled to `size` physical px, for an icon outside the exact band.
    pub(crate) fn quad_sized(self, pen: IVec2, size: UVec2, color: RgbaF16) -> RasterQuad {
        RasterQuad {
            size,
            ..self.quad(pen, color)
        }
    }
}

/// A raster the pool holds: its handle, its bearing and its content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlacedRaster {
    pub(crate) handle: AtlasHandle,
    pub(crate) bearing: I16Vec2,
    pub(crate) content: ContentType,
}

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s slab; narrow and `Copy` for the per-glyph hit
/// path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    pub(crate) state: SlotState,
    /// The text clock's frame it was last drawn or looked up in, which a non-drawing entry
    /// expires by.
    pub(crate) last_use: u64,
}

/// What a slab entry holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotState {
    /// A raster the pool holds, which it may evict.
    Placed(PlacedRaster),
    /// A live entry that draws nothing (whitespace, no pixels), which expires on a deadline.
    Unplaced,
    /// On the free list, named by no cache key.
    Free,
}

impl AtlasSlot {
    /// The raster the entry draws, `None` for one that draws nothing or is free.
    #[inline]
    pub(crate) const fn raster(&self) -> Option<PlacedRaster> {
        match self.state {
            SlotState::Placed(raster) => Some(raster),
            SlotState::Unplaced | SlotState::Free => None,
        }
    }
}

#[cfg(test)]
pub(super) mod internals {
    use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, SlotState};

    impl AtlasSlot {
        /// A non-drawing entry last used on `last_use`.
        pub(crate) const fn unplaced(last_use: u64) -> Self {
            Self {
                state: SlotState::Unplaced,
                last_use,
            }
        }
    }
}
