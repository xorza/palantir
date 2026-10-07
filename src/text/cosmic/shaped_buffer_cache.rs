//! The shaped-buffer cache: which buffers are resident, how long each stays, and
//! the pool an evicted one is recycled through.
//!
//! Bounded by age, not capacity: a count budget below the working set thrashes
//! (redraw is cyclic, LRU's worst case) and above it fills with widths a resize
//! drag never revisits. [`ShapedBufferCache::insert`] files a probationary ticket,
//! [`ShapedBufferCache::hit`] promotes, [`ShapedBufferCache::supersede`] demotes,
//! and [`ShapedBufferCache::tick_frame`] settles what came due; the
//! [`ExpiryWheel`] contract ([`crate::common::expiry_wheel`]) holds only if all
//! four agree.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::text::cosmic::cache_entry::{CacheEntry, CachedExtent};
use crate::text::cosmic::counters::CacheCounters;
use crate::text::key::TextShapeKey;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use cosmic_text::Buffer;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

const RECYCLE_POOL_CAP: usize = 128;

/// Frames a *probationary* entry survives before [`ShapedBufferCache::tick_frame`]
/// drops it: inserted and never looked up, or
/// [superseded](ShapedBufferCache::supersede) after its reuse slot moved to
/// another key. Short on purpose: a drag mints a never-reused key per run per
/// frame, and the protected window would hold `runs × RENDERED_RUN_KEEP_FRAMES`
/// dead buffers.
///
/// **Supersession is what makes this window reach that population.** Layout shapes
/// a run and the encoder renders it on the same frame, which is a lookup, so every
/// buffer would be promoted at creation; the measure and encoded-run caches
/// short-circuit before here, so nothing repairs that later. `TextSystem` alone
/// can tell "wants a different shape" (dead) from "left the tree" (may return). It
/// is a demotion, not an eviction: four frames of grace let an oscillating label or
/// a reversing drag still hit.
///
/// # Why not reference-counted retention
///
/// `EncodedKey` embeds [`TextShapeKey`], so a width drag mints an encoded entry per
/// frame living `ENCODED_CACHE_KEEP_FRAMES`. Holding its buffer strongly pins
/// `runs × (window + 1)` buffers, the growth this scheme stops. Holding it weakly
/// leaves buffers dying under live entries, so the restore path (`ShapedTextRef`,
/// `InternedText`, `CosmicMeasure::ensure_buffer`) must stay. The wins exclude each
/// other.
pub(crate) const PROBATION_KEEP_FRAMES: u64 = 4;

/// The longest a rendered run waits to be retired: its keep plus the stagger.
const KEEP_FRAMES: u64 = RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK;

/// A resident shaped buffer with the x its glyph block starts at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    pub(crate) left: f32,
}

/// One shaped `Buffer` per [`TextShapeKey`], on a two-tier age window: protected is
/// [`RENDERED_RUN_KEEP_FRAMES`] plus the entry's share of
/// [`RENDERED_RUN_KEEP_SPREAD_MASK`]; probation is [`PROBATION_KEEP_FRAMES`]. The
/// tier is the entry's `dies_at`.
#[derive(Debug)]
pub(super) struct ShapedBufferCache {
    entries: FxHashMap<TextShapeKey, CacheEntry>,
    /// The frame clock every text cache in the crate ages against, advanced by
    /// [`Self::tick_frame`] and stamped on every entry. Downstream caches read it
    /// through [`TextShaper::frame`](crate::text::shaper::TextShaper) instead of
    /// counting, so [`RENDERED_RUN_KEEP_FRAMES`] can state an ordering against them.
    ///
    /// It advances on record while the backend sweeps on submit, so it can jump or
    /// stall: fine for an age comparison, never a `frame % INTERVAL == 0` gate. It
    /// counts host frames: N windows painting together tick it once per round
    /// (`FrameRuntime::tick_text_clock`).
    frame: u64,
    /// Which keys come due on which frame. A wheel, not an earliest-`keep_until` gate:
    /// a key changing every frame re-pins that minimum on each insert, and every frame
    /// would walk the whole map to reclaim one entry.
    expiry: ExpiryWheel<TextShapeKey>,
    /// LIFO pool fed by eviction; `Buffer::set_text` reuses its allocations.
    recycle_pool: Vec<Buffer>,
    pub(super) counters: CacheCounters,
}

impl Default for ShapedBufferCache {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            frame: 0,
            expiry: ExpiryWheel::with_keep(KEEP_FRAMES),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            counters: CacheCounters::default(),
        }
    }
}

impl ShapedBufferCache {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) const fn frame(&self) -> u64 {
        self.frame
    }

    /// Look up the shaped run for `key`, or `None` when no buffer is resident. Absence
    /// is an answer here. Does not promote, unlike [`Self::hit`].
    pub(super) fn shaped_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        self.entries.get(&key).map(|e| ShapedRun {
            buffer: &e.buffer,
            left: e.left,
        })
    }

    /// The resident entry under `key`, its deadline pushed to the protected window.
    /// Being asked for is the evidence of reuse over scan traffic.
    pub(super) fn hit(&mut self, key: TextShapeKey) -> Option<&mut CacheEntry> {
        let entry = self.entries.get_mut(&key)?;
        entry.dies_at = self.frame + RENDERED_RUN_KEEP_FRAMES + key.keep_spread() + 1;
        self.counters.hits.bump();
        Some(entry)
    }

    /// The cached unbounded shape a truncating fit cuts from. Read once per miss by
    /// `CosmicMeasure::shape_truncated` (after restoring the key), which snapshots the
    /// glyphs since shaping needs the measurer mutably. Does not promote.
    pub(super) fn probe(&self, key: TextShapeKey) -> &CacheEntry {
        self.entries
            .get(&key)
            .expect("truncation requires the cached unbounded shape")
    }

    /// Store a freshly shaped buffer, probationary until a [`Self::hit`] promotes it.
    pub(super) fn insert(
        &mut self,
        key: TextShapeKey,
        buffer: Buffer,
        extent: CachedExtent,
        left: f32,
    ) {
        // Counted here, not per `shape_until_scroll`: one cached run is one tally even if
        // the truncation back-off reshapes several times. The ellipsis probe isn't counted.
        self.counters.shapes.bump();
        let dies_at = self.probation_dies_at();
        let ticket_seq = self.expiry.schedule(key, dies_at);
        let displaced = self.entries.insert(
            key,
            CacheEntry {
                buffer,
                extent,
                left,
                dies_at,
                ticket_seq,
            },
        );
        debug_assert!(
            displaced.is_none(),
            "every caller checks residency first, so a key is inserted once",
        );
    }

    /// The frame an entry filed into the probation window is first dead.
    const fn probation_dies_at(&self) -> u64 {
        self.frame + PROBATION_KEEP_FRAMES + 1
    }

    /// Demote `key` to the probation window: its reuse slot now answers another key.
    /// Only shortens a deadline, and files a second ticket for the earlier frame since
    /// the outstanding one sits at the retracted deadline. Silent on a non-resident key.
    pub(super) fn supersede(&mut self, key: TextShapeKey) {
        let dies_at = self.probation_dies_at();
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        self.counters.supersedes.bump();
        // Never extends a life: an entry already closer to expiry keeps its deadline.
        if entry.dies_at > dies_at {
            entry.dies_at = dies_at;
            // The new ticket is earlier, so it decides this entry's fate; stamping it retires
            // the supplanted ticket when it fires.
            entry.ticket_seq = self.expiry.schedule(key, dies_at);
        }
    }

    /// Advance the shared frame clock one frame and drop every buffer whose deadline
    /// has passed. The one place the clock moves. Cost tracks what expires. A ticket is
    /// a hint: [`Self::hit`] pushes a deadline out and files nothing, so the real
    /// `dies_at` is re-read and a live entry re-filed.
    pub(super) fn tick_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        let entries = &mut self.entries;
        let recycle_pool = &mut self.recycle_pool;
        let counters = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already: a demote leaves two tickets outstanding, and the first may have
            // evicted this entry.
            let Entry::Occupied(slot) = entries.entry(key) else {
                return None;
            };
            // Supplanted by a later `supersede`: the live ticket is still outstanding, so
            // this one dies. Re-filing it grew the ticket count for as long as the entry lived.
            if seq != slot.get().ticket_seq {
                return None;
            }
            if slot.get().dies_at > frame {
                // Re-filed under the same serial, so the entry's stamp still names it.
                return Some(slot.get().dies_at);
            }
            counters.expiries.bump();
            recycle_into(recycle_pool, slot.remove().buffer);
            None
        });
    }

    /// Drop every shaped buffer now, recycling each. Owed by `CosmicMeasure::load_font`;
    /// tests use it for a guaranteed-cold cache.
    pub(super) fn drop_all(&mut self) {
        let recycle_pool = &mut self.recycle_pool;
        for (_, entry) in self.entries.drain() {
            recycle_into(recycle_pool, entry.buffer);
        }
        self.expiry.clear();
    }

    /// A buffer to reshape into, or `None` when the caller must build one.
    pub(super) fn take_recycled(&mut self) -> Option<Buffer> {
        self.recycle_pool.pop()
    }

    /// Hand back a buffer that never became an entry (the ellipsis probe's).
    pub(super) fn recycle(&mut self, buffer: Buffer) {
        recycle_into(&mut self.recycle_pool, buffer);
    }
}

/// The pool's one write as a free function: [`ShapedBufferCache::tick_frame`] and
/// [`ShapedBufferCache::drop_all`] hold another field across it.
fn recycle_into(pool: &mut Vec<Buffer>, buffer: Buffer) {
    if pool.len() < RECYCLE_POOL_CAP {
        pool.push(buffer);
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    #[cfg(test)]
    use crate::text::cosmic::counters::CacheCounts;
    #[cfg(test)]
    use crate::text::extent::TextExtent;

    pub(crate) const RING_FRAMES: u64 = ExpiryWheel::<TextShapeKey>::slots_for_keep(KEEP_FRAMES);

    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    #[cfg(test)]
    impl ShapedBufferCache {
        /// Outstanding expiry tickets. Grows by one per demote if a supplanted ticket
        /// re-files itself.
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        pub(crate) fn counts(&self) -> CacheCounts {
            self.counters.counts()
        }

        pub(crate) fn extent(&self, key: TextShapeKey) -> Option<TextExtent> {
            self.entries.get(&key).map(|entry| entry.extent.extent())
        }

        pub(crate) fn recycle_pool_stats(&self) -> RecyclePoolStats {
            RecyclePoolStats {
                len: self.recycle_pool.len(),
                capacity: self.recycle_pool.capacity(),
                limit: RECYCLE_POOL_CAP,
            }
        }
    }
}
