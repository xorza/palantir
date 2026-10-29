//! One resident raster's placement and lifetime stamps, the atlas hit path's hot read.

use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::content_type::ContentType;
use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

/// Where a packed raster sits on its side, its bearing and its packer rectangle; a non-drawing entry owns none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlotPlacement {
    /// Top-left texel on its side.
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
    /// Pen position to the raster's top-left; `y` up.
    pub(crate) bearing: I16Vec2,
    /// Which side holds the rectangle, also the sampling mode.
    pub(crate) content: ContentType,
}

impl SlotPlacement {
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
            uv_and_kind: RasterQuad::pack_uv(self.origin.x, self.origin.y, self.content),
            color,
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

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s dense slab; narrow and `Copy` for the per-glyph hit path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    pub(crate) state: SlotState,
    /// Bumped when the index goes to another raster, so a stale encoded run is detected.
    pub(crate) generation: u32,
    /// Frame last drawn or looked up; the clock hand skips the current frame.
    pub(crate) last_use: u64,
}

/// What a slab entry holds. The enum's spare values ride in [`ContentType`]'s niche, so it is no
/// wider than its placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SlotState {
    /// A raster drawn from its rectangle, which the eviction clock can reclaim.
    Placed(SlotPlacement),
    /// A live entry that draws nothing (whitespace, no pixels), which expires on a deadline.
    Unplaced,
    /// On the free list, named by no cache key; set by
    /// [`FreeSlots::release`](super::free_slots::FreeSlots::release).
    Free,
}

impl AtlasSlot {
    /// Where the entry draws from, `None` for one that draws nothing or is free.
    #[inline]
    pub(crate) const fn placement(&self) -> Option<SlotPlacement> {
        match self.state {
            SlotState::Placed(placement) => Some(placement),
            SlotState::Unplaced | SlotState::Free => None,
        }
    }
}

#[cfg(test)]
pub(super) mod internals {
    use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement, SlotState};
    use crate::primitives::paint::content_type::ContentType;
    use glam::{I16Vec2, U16Vec2};

    impl SlotPlacement {
        /// A zero mask-side placement.
        pub(crate) const fn for_test() -> Self {
            Self {
                origin: U16Vec2::ZERO,
                size: U16Vec2::ZERO,
                bearing: I16Vec2::ZERO,
                content: ContentType::Mask,
            }
        }
    }

    impl AtlasSlot {
        /// A mask entry with a zero placement when `drawn`, none otherwise, and its stamps.
        pub(crate) fn for_test(drawn: bool, last_use: u64) -> Self {
            Self {
                state: if drawn {
                    SlotState::Placed(SlotPlacement::for_test())
                } else {
                    SlotState::Unplaced
                },
                generation: 0,
                last_use,
            }
        }
    }
}
