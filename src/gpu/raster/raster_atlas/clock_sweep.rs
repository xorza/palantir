//! The result of one turn of the eviction clock, named so the policy is testable on a hand-built slab with no device.

use crate::gpu::raster::raster_atlas::atlas_slot::AtlasSlot;
use crate::primitives::paint::content_type::ContentType;

/// One turn of [`RasterAtlas::evict_one`]'s clock: where the hand ended, what it found, how far it walked.
///
/// [`RasterAtlas::evict_one`]: super::RasterAtlas
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClockSweep {
    pub(super) victim: Option<u32>,
    /// Where the next sweep resumes: past the victim, since that slot is about to be refilled.
    pub(super) hand: u32,
    /// Slots examined, billed to [`AtlasCounters::evict_scans`](super::counters::AtlasCounters); a healthy thrash stops after one or two.
    pub(super) examined: u32,
}

impl ClockSweep {
    /// Advance `hand` over `slots` to an entry that is packed, of `target` content, and not drawn on `current_frame`; gives up after one rotation. [`AtlasSlot::placement`] keeps free-listed slots out.
    pub(super) fn over(
        slots: &[AtlasSlot],
        hand: u32,
        target: ContentType,
        current_frame: u64,
    ) -> Self {
        let n = slots.len();
        if n == 0 {
            return Self {
                victim: None,
                hand: 0,
                examined: 0,
            };
        }
        // `slots` only grows, but a hand parked at the old length is possible after a `store` push: wrap it in.
        let mut at = hand as usize % n;
        for examined in 1..=n {
            let idx = at;
            at = if at + 1 == n { 0 } else { at + 1 };
            let slot = &slots[idx];
            if slot.placement.is_some_and(|p| p.content == target) && slot.last_use < current_frame
            {
                return Self {
                    victim: Some(idx as u32),
                    hand: at as u32,
                    examined: examined as u32,
                };
            }
        }
        Self {
            victim: None,
            hand: at as u32,
            examined: n as u32,
        }
    }
}
