//! The result of one turn of the eviction clock, named so the policy is testable on a hand-built slab with no device.

use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement};
use crate::primitives::paint::content_type::ContentType;

/// One turn of the eviction clock: where the hand ended, what it found, how far it walked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClockSweep {
    /// The first evictable slot that `takes` accepts.
    pub(super) victim: Option<u32>,
    /// With no such slot, whether the rotation passed an evictable one `takes` refused: a side
    /// with none is dry, and learning it costs no second rotation.
    pub(super) refused: bool,
    /// Where the next sweep resumes: past the victim, since that slot is about to be refilled.
    pub(super) hand: u32,
    /// Slots examined, billed to [`AtlasCounters::evict_scans`](super::counters::AtlasCounters); a healthy thrash stops after one or two.
    pub(super) examined: u32,
}

/// The slots a sweep may evict: packed with `content` and not drawn on `cur_frame`; a victim is
/// one placed where `takes` accepts, the rest only [`ClockSweep::refused`].
#[derive(Clone, Copy, Debug)]
pub(super) struct Evictable<F> {
    pub(super) content: ContentType,
    pub(super) cur_frame: u64,
    pub(super) takes: F,
}

impl ClockSweep {
    /// Advance `hand` over `slots` to an evictable entry `takes` accepts; gives up after one
    /// rotation. [`AtlasSlot::placement`] keeps free-listed slots out.
    pub(super) fn over(
        slots: &[AtlasSlot],
        hand: u32,
        wanted: Evictable<impl Fn(&SlotPlacement) -> bool>,
    ) -> Self {
        let Evictable {
            content: target,
            cur_frame,
            takes,
        } = wanted;
        let n = slots.len();
        if n == 0 {
            return Self {
                victim: None,
                refused: false,
                hand: 0,
                examined: 0,
            };
        }
        // `slots` only grows, but a hand parked at the old length is possible after a `store` push: wrap it in.
        let mut at = hand as usize % n;
        let mut refused = false;
        for examined in 1..=n {
            let idx = at;
            at = if at + 1 == n { 0 } else { at + 1 };
            let slot = &slots[idx];
            let Some(placement) = slot.placement() else {
                continue;
            };
            if placement.content != target || slot.last_use >= cur_frame {
                continue;
            }
            if takes(&placement) {
                return Self {
                    victim: Some(idx as u32),
                    refused,
                    hand: at as u32,
                    examined: examined as u32,
                };
            }
            refused = true;
        }
        Self {
            victim: None,
            refused,
            hand: at as u32,
            examined: n as u32,
        }
    }
}
