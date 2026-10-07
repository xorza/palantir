//! One resident raster's placement and lifetime stamps, the atlas hit path's hot read.

use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use etagere::AllocId;
use glam::{I16Vec2, IVec2, U16Vec2};

/// Where a packed raster sits on its side, its bearing and its packer rectangle; a non-drawing entry owns none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotPlacement {
    /// Top-left texel on its side.
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
    /// Pen position to the raster's top-left; `y` up.
    pub(crate) bearing: I16Vec2,
    /// Which side holds the rectangle, also the sampling mode.
    pub(crate) content: ContentType,
    pub(crate) alloc: AllocId,
}

impl SlotPlacement {
    /// The instance drawing this raster at `pen` plus the bearing.
    pub(crate) fn quad(self, pen: IVec2, color: RgbaF16) -> RasterQuad {
        let dim = self.size.to_array();
        RasterQuad {
            // Bearing `y` is up; screen `y` is down.
            pos: [
                pen.x + i32::from(self.bearing.x),
                pen.y - i32::from(self.bearing.y),
            ],
            dim,
            size: dim,
            uv_and_kind: RasterQuad::pack_uv(self.origin.x, self.origin.y, self.content),
            color,
        }
    }

    /// [`Self::quad`] resampled to `size` physical px, for an icon outside the exact band.
    pub(crate) fn quad_sized(self, pen: IVec2, size: U16Vec2, color: RgbaF16) -> RasterQuad {
        RasterQuad {
            size: size.to_array(),
            ..self.quad(pen, color)
        }
    }
}

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s dense slab; narrow and `Copy` for the per-glyph hit path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    /// Where this raster draws from, or `None` for a non-drawing entry
    /// (whitespace, no pixels), which expires on a deadline. `Some` is what
    /// lets the eviction clock reclaim it; [`FreeSlots::release`](super::free_slots::FreeSlots::release) clears it.
    pub(crate) placement: Option<SlotPlacement>,
    /// Bumped when the index goes to another raster, so a stale encoded run is detected.
    pub(crate) generation: u32,
    /// Frame last drawn or looked up; the clock hand skips the current frame.
    pub(crate) last_use: u64,
    /// On the free list. `placement` cannot say so (a live non-drawing entry is
    /// also `None`) and the list is `O(n)`. Rides in existing padding.
    pub(crate) free: bool,
}

#[cfg(test)]
pub(super) mod internals {
    use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement};
    use crate::primitives::paint::content_type::ContentType;
    use etagere::AllocId;
    use glam::{I16Vec2, U16Vec2};

    impl SlotPlacement {
        /// A zero mask-side placement carrying only `alloc`.
        pub(crate) fn for_test(alloc: AllocId) -> Self {
            Self {
                origin: U16Vec2::ZERO,
                size: U16Vec2::ZERO,
                bearing: I16Vec2::ZERO,
                content: ContentType::Mask,
                alloc,
            }
        }
    }

    impl AtlasSlot {
        /// A zero-placement mask entry carrying only the allocation and stamps.
        pub(crate) fn for_test(alloc: Option<AllocId>, last_use: u64) -> Self {
            Self {
                placement: alloc.map(SlotPlacement::for_test),
                generation: 0,
                last_use,
                free: false,
            }
        }
    }
}
