//! The shaped-buffer cache's two-tier retention: what an entry is, and
//! the four operations that move it between the windows.
//!
//! Insert files a probationary ticket, a lookup promotes, a supersede
//! demotes, and the end-of-frame sweep settles whatever came due. They
//! live together because they are one protocol — the [`ExpiryWheel`]
//! contract in [`crate::common::expiry_wheel`] is only upheld by all four
//! agreeing, and reading any one of them alone is how the ticket-leak
//! regression got written.

use crate::common::expiry_wheel::TicketSeq;
use crate::text::cosmic::geometry::ShapedGeometry;
use crate::text::cosmic::{
    CosmicMeasure, PROBATION_KEEP_FRAMES, PROTECTED_KEEP_FRAMES, recycle_buffer,
};
use crate::text::key::TextShapeKey;
use crate::text::root::TextRoot;
use cosmic_text::Buffer;
use std::collections::hash_map::Entry;

#[derive(Debug)]
pub(super) struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextShapeKey`] at render time so the
    /// text backend can build a `TextArea` without reshaping.
    pub(super) buffer: Buffer,
    /// What this buffer measured to. A bounded entry's floor is `None`
    /// and its single-line flag describes the resolve rather than the
    /// run; both are inert, since only the unbounded root's copy is ever
    /// read back.
    pub(super) root: TextRoot,
    /// x of the block's left edge in buffer space — what every reader
    /// subtracts to put the block's own origin at 0.
    ///
    /// Cosmic does not lay every line out from 0. A non-left per-line
    /// align shifts each line right by `(line_width - line_w) * factor`,
    /// and *any* RTL run in a width-bounded buffer starts at `line_width`
    /// and advances leftward (`shape.rs`'s `start_x`), alignment or not.
    /// Measuring from 0 would then count that gap as part of the run's
    /// own width, and the encoder — which aligns the measured block
    /// inside the leaf rect — would apply the offset a second time.
    ///
    /// Zero for every unbounded buffer, so only a wrapped, width-bounded
    /// run can carry a non-zero value.
    pub(super) left: f32,
    /// Last frame on which this entry is kept; [`CosmicMeasure::end_frame`]
    /// drops it once the clock passes this. Insertion sets it one
    /// probation window out and every lookup pushes it a protected window
    /// out, so the two-tier policy needs no separate "has been reused"
    /// flag — the deadline *is* the tier.
    pub(super) keep_until: u64,
    /// Serial of this entry's live expiry ticket. A ticket firing under
    /// any other one was supplanted by a later
    /// [`CosmicMeasure::supersede`] and dies in the sweep instead of
    /// re-filing itself — without which a run that is demoted and
    /// promoted each frame accumulates one permanent ticket per cycle.
    pub(super) ticket_seq: TicketSeq,
}

impl CosmicMeasure {
    /// Store a freshly shaped buffer. Entries start probationary; only a
    /// later lookup promotes them (see [`PROBATION_KEEP_FRAMES`]).
    pub(super) fn insert(&mut self, key: TextShapeKey, buffer: Buffer, geometry: ShapedGeometry) {
        // Counted here rather than per `shape_until_scroll` so one
        // cached run is one tally: `measure_truncated`'s back-off can
        // reshape a prefix several times to land inside the committed
        // width, and a workload test cares that the run was shaped, not
        // how many attempts the cut took. The memoized ellipsis probe
        // shapes without inserting and is deliberately not counted.
        self.counters.shapes.bump();
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        // First frame on which the entry is dead, matching the sweep's
        // own `keep_until < frame` test.
        let ticket_seq = self.expiry.schedule(key, keep_until + 1);
        let displaced = self.cache.insert(
            key,
            CacheEntry {
                buffer,
                root: geometry.root,
                left: geometry.left,
                keep_until,
                ticket_seq,
            },
        );
        // Unreachable today — every caller checks `cache_hit` first, so a
        // key is inserted once — but the pool must not silently leak a
        // buffer the moment a second insert path appears.
        if let Some(old) = displaced {
            recycle_buffer(&mut self.recycle_pool, old.buffer);
        }
    }

    /// A cached entry's [`TextRoot`] for `key`, or `None` on a miss.
    /// Layout hits and encoder ensures both land here, and both push the
    /// entry's deadline out to the protected window — being asked for at
    /// all is the evidence that separates reuse from scan traffic, so no
    /// separate promotion step is needed.
    pub(super) fn cache_hit(&mut self, key: TextShapeKey) -> Option<TextRoot> {
        let keep_until = self.frame + PROTECTED_KEEP_FRAMES;
        let hit = self.cache.get_mut(&key).map(|entry| {
            entry.keep_until = keep_until;
            entry.root
        });
        if hit.is_some() {
            self.counters.hits.bump();
        }
        hit
    }

    /// Demote `key` to the probation window: the reuse slot that owned
    /// it now answers a different key, so nothing can ask for it through
    /// that slot again. See [`PROBATION_KEEP_FRAMES`] for why this is
    /// the signal the two-tier policy runs on.
    ///
    /// Only ever shortens a deadline — a supersede must not extend the
    /// life of an entry already closer to expiry — and files a second
    /// ticket for the earlier frame, since the outstanding one sits at
    /// the deadline this just retracted.
    ///
    /// Silent on a key that isn't resident: the buffer may already have
    /// aged out, and superseding what is gone is a no-op, not an error.
    pub(crate) fn supersede(&mut self, key: TextShapeKey) {
        if key.is_invalid() {
            return;
        }
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        let Some(entry) = self.cache.get_mut(&key) else {
            return;
        };
        self.counters.supersedes.bump();
        // Never *extends* a life: an entry already closer to expiry —
        // one that was inserted and never looked up — keeps its own
        // deadline.
        if entry.keep_until > keep_until {
            entry.keep_until = keep_until;
            // The new ticket is earlier than the outstanding one, so it
            // is the one that decides this entry's fate: stamping it
            // here retires the supplanted ticket when it fires.
            entry.ticket_seq = self.expiry.schedule(key, keep_until + 1);
        }
    }

    /// Take the shaper's `frame` clock and drop every buffer whose
    /// deadline has passed.
    ///
    /// `frame` is only ever read, never derived from a local counter, so
    /// a clock that jumps several frames at once is handled by the same
    /// comparison as one that advances by one.
    ///
    /// Age, not capacity. A count budget cannot express what this cache
    /// needs: set below the live working set it thrashes — UI redraw is a
    /// cyclic access pattern, LRU's worst case, so the overflow misses
    /// every frame forever — and set above it, a resize drag fills it with
    /// widths that can never be hit again. Ageing bounds both without a
    /// number to guess: an app keeps exactly what it keeps touching, and
    /// scan traffic falls out on its own.
    ///
    /// Cost tracks what expires, not what is resident: [`Self::expiry`]
    /// hands back only the keys whose ticket came due, so a frame holding
    /// a scrolled document's whole working set pays the same as an empty
    /// one unless something actually lapsed.
    ///
    /// A ticket is a hint, never authority to drop. Deadlines move after
    /// it is filed — [`Self::cache_hit`] pushes one out and deliberately
    /// files nothing, which is what keeps a re-read entry from filing a
    /// ticket per frame — so the real `keep_until` is re-read here and a
    /// still-live entry is simply re-filed.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        debug_assert!(frame >= self.frame, "the shared frame clock ran backwards");
        self.frame = frame;
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        let probe = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already — a demote leaves two tickets outstanding
            // and both can come due in one drain, so whichever settled
            // first may have evicted the entry this one is holding.
            let Entry::Occupied(slot) = cache.entry(key) else {
                return None;
            };
            // Supplanted by a later `supersede`: the entry's live ticket
            // is still outstanding and will settle it, so this one is
            // surplus and dies here. Re-filing it instead is what let the
            // per-entry ticket count — and with it the per-frame drain —
            // grow for as long as the entry stayed resident.
            if seq != slot.get().ticket_seq {
                return None;
            }
            if slot.get().keep_until >= frame {
                // Re-filed under the same serial, so the entry's stamp
                // still names it and nothing has to be written back.
                return Some(slot.get().keep_until + 1);
            }
            probe.expiries.bump();
            recycle_buffer(recycle_pool, slot.remove().buffer);
            None
        });
    }
}
