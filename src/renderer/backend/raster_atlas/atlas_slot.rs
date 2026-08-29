//! One resident raster's placement, bearing and lifetime stamps — the hot
//! read on the atlas hit path.

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use etagere::AllocId;

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
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) left: i16,
    pub(crate) top: i16,
    pub(crate) content: ContentType,
    /// The packer rectangle this raster owns, or `None` for a
    /// non-drawing entry — a whitespace glyph, or one the rasterizer
    /// produced no pixels for. The clock can only reclaim the first kind;
    /// the second expires on a deadline instead.
    pub(crate) alloc: Option<AllocId>,
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
    /// `alloc` cannot answer it: a non-drawing entry carries `None` while
    /// still live, so the two are indistinguishable there. Kept as a
    /// field rather than asked of the list, which answers the same
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

impl AtlasSlot {
    /// Whether the eviction clock may take this slot: it owns a packer
    /// rectangle, so it is neither a non-drawing entry nor an index
    /// already on the free list.
    ///
    /// The second half is the load-bearing one, and it holds because
    /// [`FreeSlots::release`](super::free_slots::FreeSlots::release)
    /// clears `alloc` on the way onto that list.
    pub(super) fn is_packed(&self) -> bool {
        self.alloc.is_some()
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use crate::renderer::backend::raster_atlas::atlas_slot::AtlasSlot;
    use crate::renderer::backend::raster_atlas::content_type::ContentType;
    use etagere::AllocId;

    impl AtlasSlot {
        /// A zero-placement mask entry, for the tests that care only
        /// about `alloc` and the two stamps.
        pub(crate) fn for_test(alloc: Option<AllocId>, last_use: u64) -> Self {
            Self {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                content: ContentType::Mask,
                alloc,
                generation: 0,
                last_use,
                free: false,
            }
        }
    }
}
