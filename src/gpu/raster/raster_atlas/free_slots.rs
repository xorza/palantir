//! Slab indices no cache key names, awaiting the next insert.

use crate::gpu::raster::raster_atlas::atlas_slot::AtlasSlot;
use crate::gpu::raster::raster_atlas::side::Side;

/// Slab indices no cache key names. `release` is the only way in: a listed index has no
/// [`AtlasSlot::placement`], which `ClockSweep` picks victims by, so it is never released twice.
#[derive(Debug, Default)]
pub(super) struct FreeSlots(Vec<u32>);

impl FreeSlots {
    /// Returns `idx`'s rectangle to its packer and the index to this list, bumping the generation so
    /// encoded runs read it as stale (skipped for non-drawing entries, which own no rectangle).
    pub(super) fn release(&mut self, slots: &mut [AtlasSlot], sides: &mut [Side], idx: u32) {
        let slot = &mut slots[idx as usize];
        debug_assert!(
            !slot.free,
            "slab index {idx} released twice; two keys would share one slot",
        );
        slot.free = true;
        if let Some(placement) = slot.placement.take() {
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("glyph slot generation overflowed");
            sides[placement.content as usize]
                .packer
                .deallocate(placement.alloc);
        }
        self.0.push(idx);
    }

    pub(super) fn claim(&mut self) -> Option<u32> {
        self.0.pop()
    }
}

#[cfg(test)]
pub(super) mod internals {
    use crate::gpu::raster::raster_atlas::free_slots::FreeSlots;

    impl FreeSlots {
        pub(crate) fn as_slice(&self) -> &[u32] {
            &self.0
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::raster::raster_atlas::atlas_slot::AtlasSlot;
    use crate::gpu::raster::raster_atlas::free_slots::FreeSlots;

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

    /// A double release would hand one slot to two live keys. Debug-only.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "released twice")]
    fn releasing_one_index_twice_panics() {
        let mut slots = [AtlasSlot::for_test(None, 0)];
        let mut free = FreeSlots::default();
        free.release(&mut slots, &mut [], 0);
        free.release(&mut slots, &mut [], 0);
    }
}
