//! The result of one turn of the eviction clock, named so the policy is
//! testable against a hand-built slab with no device present.

use crate::primitives::content_type::ContentType;
use crate::renderer::backend::raster_atlas::atlas_slot::AtlasSlot;

/// One turn of [`RasterAtlas::evict_one`]'s clock: where the hand ended
/// up, what it found, and how far it walked.
///
/// A named result over a hand-built slab, so the policy is testable with
/// no `wgpu::Device` in sight — the hand's persistence across calls is
/// the property most worth pinning and the least visible from outside.
///
/// [`RasterAtlas::evict_one`]: super::RasterAtlas
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClockSweep {
    pub(super) victim: Option<u32>,
    /// Where the next sweep resumes. Past the victim, not on it: the
    /// slot just evicted is about to be refilled, and starting there
    /// would make the next eviction reconsider it first.
    pub(super) hand: u32,
    /// Slots examined. What
    /// [`AtlasCounters::evict_scans`](super::counters::AtlasCounters) bills,
    /// and the number that says whether the clock is behaving — a healthy
    /// thrash state stops after one or two.
    pub(super) examined: u32,
}

impl ClockSweep {
    /// Advance `hand` over `slots` until it meets an entry eligible for
    /// eviction: packed, of `target` content, and not drawn on
    /// `current_frame`. Gives up after one full rotation.
    ///
    /// [`AtlasSlot::placement`] is what keeps a slot already on the free
    /// list out of the result — see its doc.
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
        // `slots` only ever grows, but a hand parked at the old length is
        // still possible after a `store` that pushed — wrap it in.
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
