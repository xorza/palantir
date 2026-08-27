//! The slab indices no cache key names any more, waiting to be handed to
//! the next insert.

use crate::renderer::backend::raster_atlas::atlas_slot::AtlasSlot;
use crate::renderer::backend::raster_atlas::side::Side;

/// The slab indices no cache key names, waiting to be handed to the next
/// [`RasterAtlas::store`](super::RasterAtlas).
///
/// **The list is private and [`Self::release`] is the only way onto it**,
/// which is what carries the rule the eviction clock rests on: an index on
/// this list has `alloc == None`. `ClockSweep` picks victims by
/// [`AtlasSlot::is_packed`], so a free index it could see again would be
/// released a second time — and one slab slot would then answer to two
/// live keys, silently, from the next two `store` calls onward. Clearing
/// the allocation and pushing the index are one step here, so the two
/// cannot come apart.
#[derive(Debug, Default)]
pub(super) struct FreeSlots(Vec<u32>);

impl FreeSlots {
    /// Hand slab index `idx`'s resources back: its rectangle to the side's
    /// packer, and the index itself to this list.
    ///
    /// Advancing the generation is what makes the index safe to hand on:
    /// an encoded run still holding it reads the slot as stale rather than
    /// drawing whatever took its place. A non-drawing entry owns no
    /// rectangle and no run ever records its index, so the bump is skipped
    /// there — which is also why an expiring non-drawing entry releases
    /// through this same call rather than a push of its own.
    pub(super) fn release(&mut self, slots: &mut [AtlasSlot], sides: &mut [Side], idx: u32) {
        debug_assert!(
            !self.0.contains(&idx),
            "slab index {idx} released twice; two keys would share one slot",
        );
        let slot = &mut slots[idx as usize];
        if let Some(id) = slot.alloc.take() {
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("glyph slot generation overflowed");
            sides[slot.content as usize].packer.deallocate(id);
        }
        self.0.push(idx);
    }

    /// Take an index back for reuse, or `None` when the slab must grow.
    pub(super) fn claim(&mut self) -> Option<u32> {
        self.0.pop()
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use crate::renderer::backend::raster_atlas::free_slots::FreeSlots;

    impl FreeSlots {
        /// The indices waiting for reuse, in release order.
        pub(crate) fn as_slice(&self) -> &[u32] {
            &self.0
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::raster_atlas::atlas_slot::AtlasSlot;
    use crate::renderer::backend::raster_atlas::free_slots::FreeSlots;

    // Every fixture below is non-drawing, so `release` never reaches for a
    // packer and `sides` can be empty.

    /// Released indices come back newest-first, and a non-drawing entry
    /// keeps its generation — no run ever recorded its index, so nothing
    /// has to read it as stale.
    #[test]
    fn released_indices_are_claimed_newest_first() {
        let mut slots = [AtlasSlot::for_test(None, 0), AtlasSlot::for_test(None, 0)];
        let mut free = FreeSlots::default();

        free.release(&mut slots, &mut [], 0);
        free.release(&mut slots, &mut [], 1);

        assert_eq!(free.as_slice(), [0, 1]);
        assert_eq!(free.claim(), Some(1));
        assert_eq!(free.claim(), Some(0));
        assert_eq!(free.claim(), None);
        assert_eq!(slots[0].generation, 0);
        assert_eq!(slots[1].generation, 0);
    }

    /// The failure this type exists to make loud. Two releases of one
    /// index put it on the list twice, and the next two `store` calls
    /// would hand one slab slot to two live keys — no panic, no wrong
    /// pixel until both draw.
    #[test]
    #[should_panic(expected = "released twice")]
    fn releasing_one_index_twice_panics() {
        let mut slots = [AtlasSlot::for_test(None, 0)];
        let mut free = FreeSlots::default();
        free.release(&mut slots, &mut [], 0);
        free.release(&mut slots, &mut [], 0);
    }
}
