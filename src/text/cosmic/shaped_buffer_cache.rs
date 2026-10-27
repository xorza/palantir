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
use crate::text::cosmic::text_cut::TextCut;
use crate::text::key::TextShapeKey;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use cosmic_text::Buffer;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Frames a pooled buffer must sit untouched before the pool lets it go. Long against the pauses
/// in a resize drag, so a drag that resumes finds its buffers; short against a session, so a burst
/// of text does not hold its peak for good.
const POOL_TRIM_PERIOD_FRAMES: u64 = 600;

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

/// A freshly shaped buffer, as [`ShapedBufferCache::insert`] files it; the
/// fields mean what [`CacheEntry`]'s do.
#[derive(Debug)]
pub(super) struct ShapedBuffer {
    pub(super) buffer: Buffer,
    pub(super) extent: CachedExtent,
    pub(super) left: f32,
    pub(super) cut: Option<TextCut>,
    pub(super) source: String,
}

/// An evicted entry's allocations, as the pool hands them back to be shaped into.
#[derive(Debug)]
pub(super) struct Recycled {
    pub(super) buffer: Buffer,
    /// Empty, with the capacity of the source it last held.
    pub(super) source: String,
}

impl Recycled {
    fn of(entry: CacheEntry) -> Self {
        let CacheEntry {
            buffer, mut source, ..
        } = entry;
        source.clear();
        Self { buffer, source }
    }
}

/// A resident shaped buffer with the x its glyph block starts at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    pub(crate) left: f32,
    /// The source bytes the buffer shows, when a truncating fit cut it.
    pub(crate) cut: Option<TextCut>,
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
    recycle_pool: RecyclePool,
    pub(super) counters: CacheCounters,
}

impl Default for ShapedBufferCache {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            frame: 0,
            expiry: ExpiryWheel::with_keep(KEEP_FRAMES),
            recycle_pool: RecyclePool::default(),
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

    /// Look up the shaped run of `text` under `key`, or `None` when no buffer of it is resident.
    /// Absence is an answer here. Does not promote, unlike [`Self::hit`].
    pub(super) fn shaped_run(&self, key: TextShapeKey, text: &str) -> Option<ShapedRun<'_>> {
        self.entries
            .get(&key)
            .filter(|e| e.holds(text))
            .map(CacheEntry::run)
    }

    /// The run resident under `key`, whatever text it holds: for a caller that checked the text
    /// through [`Self::shaped_run`] or [`Self::hit`] and held the cache since.
    pub(super) fn resident_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        self.entries.get(&key).map(CacheEntry::run)
    }

    /// The resident entry of `text` under `key`, its deadline pushed to the protected window.
    /// Being asked for is the evidence of reuse over scan traffic. An entry shaped from other
    /// text under the same key is evicted, so the caller's miss files its own.
    pub(super) fn hit(&mut self, key: TextShapeKey, text: &str) -> Option<&mut CacheEntry> {
        let Entry::Occupied(slot) = self.entries.entry(key) else {
            return None;
        };
        if !slot.get().holds(text) {
            self.counters.collisions.bump();
            self.recycle_pool.push(Recycled::of(slot.remove()));
            return None;
        }
        let entry = slot.into_mut();
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

    /// File `shaped` under `key`, probationary until a [`Self::hit`] promotes it.
    pub(super) fn insert(&mut self, key: TextShapeKey, shaped: ShapedBuffer) {
        let ShapedBuffer {
            buffer,
            extent,
            left,
            cut,
            source,
        } = shaped;
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
                cut,
                dies_at,
                ticket_seq,
                source,
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
            // this one dies. Re-filing it would grow the ticket count for as long as the entry
            // lived.
            if seq != slot.get().ticket_seq {
                return None;
            }
            if slot.get().dies_at > frame {
                // Re-filed under the same serial, so the entry's stamp still names it.
                return Some(slot.get().dies_at);
            }
            counters.expiries.bump();
            recycle_pool.push(Recycled::of(slot.remove()));
            None
        });
        self.recycle_pool.tick(frame);
    }

    /// Drop every shaped buffer now, recycling each. Owed by `CosmicMeasure::load_font`;
    /// tests use it for a guaranteed-cold cache.
    pub(super) fn drop_all(&mut self) {
        let recycle_pool = &mut self.recycle_pool;
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each buffer returns to the pool alone, so the order changes nothing"
        )]
        for (_, entry) in self.entries.drain() {
            recycle_pool.push(Recycled::of(entry));
        }
        self.expiry.clear();
    }

    /// A buffer to reshape into, or `None` when the caller must build one.
    pub(super) fn take_recycled(&mut self) -> Option<Recycled> {
        self.recycle_pool.pop()
    }

    /// Hand back allocations that never became an entry (the ellipsis probe's).
    pub(super) fn recycle(&mut self, recycled: Recycled) {
        self.recycle_pool.push(recycled);
    }
}

/// LIFO pool fed by eviction; `Buffer::set_text` reuses its allocations.
///
/// Uncapped, so it never holds more than the cache's peak: a buffer is built only when the pool
/// is empty, so every buffer was an entry at once. A cap below the peak would drop buffers that
/// the next frame of a drag builds again. It trims what goes cold instead, as Go's `sync.Pool`
/// drops a victim generation: the buffers the pool never dipped into over a
/// [`POOL_TRIM_PERIOD_FRAMES`] period are surplus, and go one per frame over the next, so no frame
/// frees a heap.
#[derive(Debug, Default)]
struct RecyclePool {
    buffers: Vec<Recycled>,
    /// The fewest buffers the pool held this period: that many sat untouched through it.
    low_water: usize,
    /// Buffers the last period left untouched and not yet dropped.
    surplus: usize,
    /// The frame the current period ends on.
    period_ends: u64,
}

impl RecyclePool {
    fn push(&mut self, recycled: Recycled) {
        self.buffers.push(recycled);
    }

    fn pop(&mut self) -> Option<Recycled> {
        let buffer = self.buffers.pop();
        self.low_water = self.low_water.min(self.buffers.len());
        buffer
    }

    /// Close the period once `frame` reaches its end, then drop one surplus buffer. Compared, not
    /// counted, as the frame clock can jump.
    fn tick(&mut self, frame: u64) {
        if frame >= self.period_ends {
            self.surplus = self.low_water;
            self.low_water = self.buffers.len();
            self.period_ends = frame + POOL_TRIM_PERIOD_FRAMES;
        }
        if self.surplus > 0 && self.pop().is_some() {
            self.surplus -= 1;
        }
    }
}

#[cfg(feature = "internals")]
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
    pub(crate) const POOL_TRIM_FRAMES: u64 = POOL_TRIM_PERIOD_FRAMES;

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

        pub(crate) fn recycle_pool_len(&self) -> usize {
            self.recycle_pool.buffers.len()
        }
    }
}
