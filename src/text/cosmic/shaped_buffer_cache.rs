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
//!
//! A shape outlives its size. Shaping runs unhinted, so cosmic-text's glyphs are in em
//! units and a [`TextShapeKey`] of another size, leading or width shapes the same text
//! alike: [`ShapedBufferCache::donor_lines`] hands a miss the shaped lines of a resident
//! buffer of its identity, or of one that expired, which a [`ShapeStore`] keeps, to lay out
//! at its own metrics, as cosmic-text's own `Buffer::set_metrics` lays out again and shapes
//! nothing. A theme transition moves every font size on most frames, and a run the layout
//! measures only when its size moves outlives the probation window of the buffer it shaped,
//! so an expired buffer leaves its lines for as long as a rendered run is kept. Refused:
//! cosmic-text's `shape-run-cache`, which builds a `String` key for every run it looks up
//! and trims by walking its whole map each frame.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::text::cosmic::cache_entry::{CacheEntry, ShapedBuffer};
use crate::text::cosmic::counters::CacheCounters;
use crate::text::cosmic::shape_store::ShapeStore;
use crate::text::cosmic::text_cut::TextCut;
use crate::text::key::TextShapeKey;
use crate::text::{PROBATION_KEEP_FRAMES, RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use cosmic_text::{Buffer, BufferLine};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Frames a pooled buffer must sit untouched before the pool lets it go. Long against the pauses
/// in a resize drag, so a drag that resumes finds its buffers; short against a session, so a burst
/// of text does not hold its peak for good.
pub(crate) const POOL_TRIM_PERIOD_FRAMES: u64 = 600;

/// The longest a rendered run waits to be retired: its keep plus the stagger.
pub(crate) const KEEP_FRAMES: u64 = RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK;

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
            shaped: ShapedBuffer {
                buffer, mut source, ..
            },
            ..
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
    /// Where a miss finds a shape of its text at other metrics.
    shapes: ShapeStore,
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
            shapes: ShapeStore::default(),
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
            .map(|entry| &entry.shaped)
            .filter(|shaped| shaped.holds(text))
            .map(ShapedBuffer::run)
    }

    /// The resident entry of `text` under `key`, its deadline pushed to the protected window.
    /// Being asked for is the evidence of reuse over scan traffic. An entry shaped from other
    /// text under the same key is evicted, so the caller's miss files its own.
    pub(super) fn hit(&mut self, key: TextShapeKey, text: &str) -> Option<&mut CacheEntry> {
        let Entry::Occupied(slot) = self.entries.entry(key) else {
            return None;
        };
        if !slot.get().shaped.holds(text) {
            self.counters.collisions.bump();
            let entry = slot.remove();
            if entry.shaped.cut.is_none() {
                self.shapes.left(key);
            }
            self.recycle_pool.push(Recycled::of(entry));
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

    /// The lines of a buffer that shaped `text` whole under `key`'s identity, to be laid out
    /// again: a resident one of another size or leading, else, with none resident, the one an
    /// expired entry left, at other metrics or under `key` itself. A shape at `key`'s metrics and
    /// another width answers `None`, as a resize drag shapes into a recycled buffer's
    /// allocations, where a copy of the lines allocates each of them.
    pub(super) fn donor_lines(&mut self, key: TextShapeKey, text: &str) -> Option<&[BufferLine]> {
        if let Some(donor) = self.shapes.donor(key)
            && let Some(entry) = self.entries.get(&donor)
            && entry.shaped.cut.is_none()
            && entry.shaped.holds(text)
        {
            return (!donor.shares_metrics(key)).then_some(entry.shaped.buffer.lines.as_slice());
        }
        self.shapes.kept_lines(key, text, self.frame)
    }

    /// File `shaped` under `key`, probationary until a [`Self::hit`] promotes it.
    pub(super) fn insert(&mut self, key: TextShapeKey, shaped: ShapedBuffer) {
        // Counted here, not per `shape_until_scroll`: one cached run is one tally even if
        // the truncation back-off reshapes several times. The ellipsis probe isn't counted.
        self.counters.shapes.bump();
        let dies_at = self.probation_dies_at();
        let ticket_seq = self.expiry.schedule(key, dies_at);
        if shaped.cut.is_none() {
            self.shapes.filed(key);
        }
        let displaced = self.entries.insert(
            key,
            CacheEntry {
                shaped,
                dies_at,
                ticket_seq,
            },
        );
        debug_assert!(
            displaced.is_none(),
            "every caller checks residency first, so a key is inserted once",
        );
        self.reserve_for_expiry();
    }

    /// Room for every resident buffer to expire into the store and on to the pool, charged to a
    /// frame that shapes, as an expiry comes on a later frame that need not.
    fn reserve_for_expiry(&mut self) {
        let resident = self.entries.len();
        self.shapes.reserve_kept(resident);
        self.recycle_pool
            .reserve_for(resident + self.shapes.kept_len());
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
        let shapes = &mut self.shapes;
        let recycle_pool = &mut self.recycle_pool;
        let counters = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already: a demote leaves two tickets outstanding, and the first may have
            // evicted this entry. `get`, not `entry`, which reserves on a vacant key and could
            // rehash the map on a frame that shapes nothing.
            let entry = entries.get(&key)?;
            // Supplanted by a later `supersede`: the live ticket is still outstanding, so
            // this one dies. Re-filing it would grow the ticket count for as long as the entry
            // lived.
            if seq != entry.ticket_seq {
                return None;
            }
            if entry.dies_at > frame {
                // Re-filed under the same serial, so the entry's stamp still names it.
                return Some(entry.dies_at);
            }
            counters.expiries.bump();
            let entry = entries.remove(&key).expect("read above");
            let whole = entry.shaped.cut.is_none();
            let recycled = Recycled::of(entry);
            if !whole {
                recycle_pool.push(recycled);
                return None;
            }
            shapes.left(key);
            if let Some(recycled) = shapes.keep(key, recycled, frame) {
                recycle_pool.push(recycled);
            }
            None
        });
        let recycle_pool = &mut self.recycle_pool;
        self.shapes
            .tick(frame, |recycled| recycle_pool.push(recycled));
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
        self.shapes.drain(|recycled| recycle_pool.push(recycled));
        self.expiry.clear();
    }

    /// A buffer to reshape into, or `None` when the caller must build one.
    pub(super) fn take_recycled(&mut self) -> Option<Recycled> {
        self.recycle_pool.pop()
    }

    /// Hand back allocations that never became an entry (the ellipsis probe's).
    pub(super) fn recycle(&mut self, recycled: Recycled) {
        self.recycle_pool.reserve_for(1);
        self.recycle_pool.push(recycled);
        self.reserve_for_expiry();
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
    /// Pool `recycled` in the room [`Self::reserve_for`] made, so an expiry allocates nothing.
    fn push(&mut self, recycled: Recycled) {
        debug_assert!(
            self.buffers.len() < self.buffers.capacity(),
            "the frame that shaped this buffer reserved its room in the pool",
        );
        self.buffers.push(recycled);
    }

    /// Room for `count` more buffers than the pool holds.
    fn reserve_for(&mut self, count: usize) {
        self.buffers.reserve(count);
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

#[cfg(test)]
pub(crate) mod internals {
    use super::*;
    use crate::common::counters::CounterSet;
    use crate::text::cosmic::counters::CacheCounts;
    use crate::text::extent::TextExtent;

    impl ShapedBufferCache {
        /// The entries the map has room for before it grows.
        pub(crate) fn entries_capacity(&self) -> usize {
            self.entries.capacity()
        }

        /// The buffers the store has room to keep.
        pub(crate) fn kept_capacity(&self) -> usize {
            self.shapes.kept_capacity()
        }

        /// The buffers the pool has room to hold.
        pub(crate) fn recycle_pool_capacity(&self) -> usize {
            self.recycle_pool.buffers.capacity()
        }

        /// How many buffers the store keeps.
        pub(crate) fn kept_len(&self) -> usize {
            self.shapes.kept_len()
        }

        /// The run resident under `key`, whatever text it holds, for a test that watches
        /// residency.
        pub(crate) fn resident_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
            self.entries.get(&key).map(|entry| entry.shaped.run())
        }

        /// Outstanding expiry tickets. Grows by one per demote if a supplanted ticket
        /// re-files itself.
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        pub(crate) fn counts(&self) -> CacheCounts {
            self.counters.counts()
        }

        pub(crate) fn extent(&self, key: TextShapeKey) -> Option<TextExtent> {
            self.entries
                .get(&key)
                .map(|entry| entry.shaped.extent.extent())
        }

        pub(crate) fn recycle_pool_len(&self) -> usize {
            self.recycle_pool.buffers.len()
        }
    }
}
