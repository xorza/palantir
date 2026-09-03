//! One resident raster's placement, bearing and lifetime stamps — the hot
//! read on the atlas hit path.

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use etagere::AllocId;
use glam::{I16Vec2, IVec2, U16Vec2};

/// Where a packed raster sits on its side, what bearing it draws with, and
/// the packer rectangle it owns.
///
/// One value rather than seven fields on [`AtlasSlot`], because they mean
/// something together or not at all: a non-drawing entry owns no
/// rectangle, so it has no side to name, no extent and no bearing. Every
/// reader already gated on the allocation before touching any of them, and
/// this is that gate spelled once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotPlacement {
    /// Top-left texel of the rectangle on its side.
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
    /// Offset from the pen position to the raster's top-left, in the
    /// rasterizer's sense: `x` right, `y` **up**.
    pub(crate) bearing: I16Vec2,
    /// Which side holds the rectangle — also the sampling mode the quad
    /// draws with.
    pub(crate) content: ContentType,
    pub(crate) alloc: AllocId,
}

impl SlotPlacement {
    /// The instance that draws this raster with its top-left at `pen`
    /// plus the bearing, tinted `color`.
    ///
    /// Both tenants build a quad from a slot, and every term but the pen
    /// and the tint is the slot's: the extents, the atlas origin, the
    /// side to sample.
    pub(crate) fn quad(self, pen: IVec2, color: u32) -> RasterQuad {
        RasterQuad {
            // `y` up in the rasterizer's sense, `y` down on screen.
            pos: [
                pen.x + i32::from(self.bearing.x),
                pen.y - i32::from(self.bearing.y),
            ],
            dim: RasterQuad::dim(self.size.x, self.size.y),
            uv_and_kind: RasterQuad::pack_uv(self.origin.x, self.origin.y, self.content),
            color,
        }
    }
}

/// One entry of [`RasterAtlas`](super::RasterAtlas)'s dense slab: where a
/// raster sits on its side, what bearing it draws with, and the two stamps
/// that decide when it may be taken away.
///
/// Kept narrow and `Copy` because it is the hot read: the encoded-run
/// cache's hit path loads one per glyph and copies it whole, which is why
/// the key that maps to it lives in a parallel column instead of a field
/// here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    /// Where this raster draws from, or `None` for a non-drawing entry —
    /// a whitespace glyph, or one the rasterizer produced no pixels for.
    ///
    /// Also what the eviction clock picks by: it may only reclaim an
    /// entry that owns a rectangle, so `Some` means neither a non-drawing
    /// entry nor an index already on the free list. The second half holds
    /// because [`FreeSlots::release`](super::free_slots::FreeSlots::release)
    /// clears this on the way onto that list. A non-drawing entry expires
    /// on a deadline instead.
    pub(crate) placement: Option<SlotPlacement>,
    /// Advanced whenever the slab index is handed to another raster, so
    /// an encoded run still holding the index reads it as stale rather
    /// than drawing whatever took its place.
    pub(crate) generation: u32,
    /// Frame this entry was last drawn or looked up on, in its atlas's
    /// own clock. The clock hand skips anything stamped with the current
    /// frame, and a non-drawing entry's deadline is measured from here.
    pub(crate) last_use: u64,
    /// This index is on the free list, waiting to be handed to the next
    /// insert.
    ///
    /// `placement` cannot answer it: a non-drawing entry carries `None`
    /// while still live, so the two are indistinguishable there. Kept as
    /// a field rather than asked of the list, which answers the same
    /// question in `O(n)` over every waiting index — see
    /// [`FreeSlots::release`](super::free_slots::FreeSlots::release).
    ///
    /// Carried in every profile although only a debug build reads it: it
    /// rides in padding the slot already had, so the hot copy is
    /// unchanged, and one byte costs less than the four `#[cfg]`s a
    /// debug-only field would put across this struct's three
    /// construction sites.
    pub(crate) free: bool,
}

#[cfg(test)]
pub(super) mod test_support {
    use crate::renderer::backend::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement};
    use crate::renderer::backend::raster_atlas::content_type::ContentType;
    use etagere::AllocId;
    use glam::{I16Vec2, U16Vec2};

    impl SlotPlacement {
        /// A zero placement on the mask side, for the tests that care
        /// only about the allocation it carries.
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
        /// A zero-placement mask entry, for the tests that care only
        /// about the allocation and the two stamps.
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
