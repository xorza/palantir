//! The shaped-buffer cache: which buffers are resident, how long each
//! one stays, and the pool an evicted one is recycled through.
//!
//! Bounded by **age, not capacity**. A count budget cannot express what
//! this needs: set below the live working set it thrashes — UI redraw is
//! a cyclic access pattern, LRU's worst case, so the overflow misses
//! every frame forever — and set above it, a resize drag fills it with
//! widths that can never be hit again. Ageing bounds both without a
//! number to guess: an app keeps exactly what it keeps touching, and
//! scan traffic falls out on its own.
//!
//! Four operations move an entry between the two windows, and they are
//! one protocol rather than four methods: [`ShapedBufferCache::insert`]
//! files a probationary ticket, [`ShapedBufferCache::hit`] promotes,
//! [`ShapedBufferCache::supersede`] demotes, and
//! [`ShapedBufferCache::tick_frame`] settles whatever came due. The
//! [`ExpiryWheel`] contract in [`crate::common::expiry_wheel`] is upheld
//! only by all of them agreeing, and reading any one alone is how the
//! ticket-leak regression got written.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::text::cosmic::cache_entry::{CacheEntry, CachedExtent};
use crate::text::cosmic::counters::CacheCounters;
use crate::text::key::TextShapeKey;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use cosmic_text::Buffer;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

const RECYCLE_POOL_CAP: usize = 128;

/// Frames a *probationary* entry survives before
/// [`ShapedBufferCache::tick_frame`] drops it: one inserted and never
/// looked up, or one [superseded](ShapedBufferCache::supersede) after its
/// reuse slot moved to a different key.
///
/// Short on purpose. This population is scan traffic: a resize or zoom
/// drag quantizes to a new whole-pixel wrap width nearly every frame, so
/// each run mints a key that will never be asked for again. Holding those
/// for the protected window lets one drag accumulate
/// `runs × RENDERED_RUN_KEEP_FRAMES` dead buffers.
///
/// **Supersession is what makes this window reach that population.**
/// Insertion alone does not: layout shapes a run and the encoder renders
/// it on the *same* frame, and that render is a lookup, so every drawn
/// buffer would otherwise be promoted the moment it was created and the
/// probation tier would be inert. Steady state cannot repair that by
/// re-touching it either — the measure cache and the encoded-run cache
/// both short-circuit before reaching here, so a resident buffer is
/// never looked up again on a later frame. `TextSystem` holds the only
/// signal that distinguishes "this run wants a different shape now"
/// (drag, typing, animation — dead) from "this run left the tree"
/// (scrolled away — may well return), and it reports the first through
/// [`ShapedBufferCache::supersede`].
///
/// A demotion, not an eviction: four frames of grace means a label
/// oscillating between two keys, or a drag reversing back through a
/// width it just used, still hits.
///
/// # Why not reference-counted retention
///
/// Letting an entry die when no upper cache still holds it reads like
/// the obvious replacement for this whole scheme — no windows, no
/// demotion signal. It does not work, for a measured reason.
/// `EncodedKey` embeds [`TextShapeKey`], so a width drag mints a fresh
/// encoded entry every frame and those live
/// `ENCODED_CACHE_KEEP_FRAMES` — a shorter window than this one, but
/// still one with no probation tier under it. An encoded entry holding
/// its buffer *strongly* therefore pins `runs × (that window + 1)` of
/// them, an order of magnitude past what this window achieves, and the
/// exact growth it was added to stop. Holding it *weakly* keeps the drag
/// bounded but leaves buffers dying under live encoded entries, so the
/// whole restore path (`ShapedTextRef`, `InternedText`,
/// `CosmicMeasure::ensure_buffer`) has to stay — and deleting that was
/// the other half of the idea. The two wins are mutually exclusive.
pub(crate) const PROBATION_KEEP_FRAMES: u64 = 4;

/// A resident shaped buffer paired with the x its glyph block starts at,
/// so every reader normalizes the same way off one lookup.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    /// See [`CacheEntry::left`].
    pub(crate) left: f32,
}

/// One shaped `Buffer` per [`TextShapeKey`], on a two-tier age window.
///
/// The protected tier is [`RENDERED_RUN_KEEP_FRAMES`] plus the entry's
/// own share of [`RENDERED_RUN_KEEP_SPREAD_MASK`]; the probation tier is
/// [`PROBATION_KEEP_FRAMES`]. Which tier an entry sits in is its
/// `dies_at` and nothing else — see [`CacheEntry::dies_at`].
#[derive(Debug)]
pub(super) struct ShapedBufferCache {
    entries: FxHashMap<TextShapeKey, CacheEntry>,
    /// **The** frame clock every text cache in the crate ages against,
    /// advanced by [`Self::tick_frame`]. Stamped onto every entry this
    /// touches, and the reference point both retention windows measure
    /// back from.
    ///
    /// One counter rather than one per cache. The renderer's
    /// encoded-run cache and its glyph atlas receive this reading
    /// through [`TextShaper::frame`](crate::text::shaper::TextShaper)
    /// instead of counting for themselves, which is what lets
    /// [`RENDERED_RUN_KEEP_FRAMES`] state an ordering against them at
    /// all — comparing two windows means something only while both
    /// count the same thing.
    ///
    /// It advances on the record path while the backend sweeps on the
    /// submit path, so it both jumps — two windows record before one
    /// submit — and stalls — two submits inside one recorded frame.
    /// That is fine for an age comparison. It is never a cadence gate
    /// written as `frame % INTERVAL == 0`.
    ///
    /// **It counts window-frames, not host frames — a known limit.**
    /// Every window ticks it once per frame it records, so N windows
    /// painting together age everything in units of N: two animating
    /// windows give a buffer promised 120 frames 60 of the host's. The
    /// ordering [`RENDERED_RUN_KEEP_FRAMES`] states survives, because all
    /// three windows read this one counter and shrink by the same factor
    /// — what it costs is reshaping and re-rasterizing sooner than the
    /// constants read as. A window painting at a lower rate than its
    /// sibling pays the same way: its atlas slots become evictable
    /// against the sibling's frames, so pressure reclaims them earlier
    /// than its own cadence would.
    ///
    /// Closing it takes a host-frame signal this layer does not have.
    /// Winit's `about_to_wait` is the boundary, but the tick would have
    /// to leave `FrameCycle::run` — which owns it precisely so no frame
    /// plan can skip or double it — for every driver of a window, and a
    /// driver that missed it would stall the clock rather than double
    /// it. A stalled clock leaves the glyph atlas unable to reclaim
    /// anything, with no path back; see
    /// [`TextShaper::tick_frame`](crate::text::shaper::TextShaper::tick_frame).
    frame: u64,
    /// Which keys come due on which frame, so [`Self::tick_frame`] costs
    /// what expires rather than what is resident.
    ///
    /// A wheel rather than a single earliest-`keep_until` gate, which is
    /// O(1) only while nothing churns: one key that changes every frame
    /// — a clock, an FPS counter, a scrubbing value — re-pins that
    /// minimum a probation window out on every insert, the gate stops
    /// firing, and every frame walks the whole map to reclaim one entry.
    /// The churn that would motivate such a gate is precisely the churn
    /// that defeats it.
    expiry: ExpiryWheel<TextShapeKey>,
    /// LIFO pool fed by eviction. `Buffer::set_text` reclaims its line,
    /// shaping, and layout allocations when the buffer is reset.
    recycle_pool: Vec<Buffer>,
    /// Shape / hit / supersede / expire tallies. Zero-sized outside
    /// tests.
    pub(super) counters: CacheCounters,
}

impl Default for ShapedBufferCache {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            frame: 0,
            expiry: ExpiryWheel::with_keep(
                RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK,
            ),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            counters: CacheCounters::default(),
        }
    }
}

impl ShapedBufferCache {
    /// Shaped buffers currently resident.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The current reading of the shared frame clock — see
    /// [`Self::frame`]. Every downstream cache stamps and expires
    /// against this rather than counting frames of its own.
    pub(super) fn frame(&self) -> u64 {
        self.frame
    }

    /// Look up the shaped run for `key`, or `None` when no buffer is
    /// resident under it — never measured on this cache, or aged out
    /// since.
    ///
    /// A lookup, so absence is an answer rather than a wiring bug: the
    /// probe path takes it for a run that was never shaped, and a
    /// residency check is the question itself. It does **not** promote,
    /// which is what separates it from [`Self::hit`].
    pub(super) fn shaped_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        self.entries.get(&key).map(|e| ShapedRun {
            buffer: &e.buffer,
            left: e.left,
        })
    }

    /// The resident entry under `key`, its deadline pushed out to the
    /// protected window and the hit counted.
    ///
    /// Being asked for at all is the evidence that separates reuse from
    /// scan traffic, so no separate promotion step is needed.
    pub(super) fn hit(&mut self, key: TextShapeKey) -> Option<&mut CacheEntry> {
        let entry = self.entries.get_mut(&key)?;
        entry.dies_at = self.frame + RENDERED_RUN_KEEP_FRAMES + key.keep_spread() + 1;
        self.counters.hits.bump();
        Some(entry)
    }

    /// The cached unbounded shape a truncating fit cuts from.
    ///
    /// `CosmicMeasure::shape_truncated` restores this key before reaching
    /// for it, and reads it once per miss: the shaping between back-off
    /// rounds needs the measurer mutably, so it snapshots the glyphs it
    /// cuts from rather than holding this borrow across the loop.
    ///
    /// Hands back the whole entry rather than its buffer: the caller
    /// wants the measured extent as well as the glyphs, and both come out
    /// of the one lookup. Does not promote — the restore that preceded it
    /// already did.
    pub(super) fn probe(&self, key: TextShapeKey) -> &CacheEntry {
        self.entries
            .get(&key)
            .expect("truncation requires the cached unbounded shape")
    }

    /// Store a freshly shaped buffer. Entries start probationary; only a
    /// later [`Self::hit`] promotes them (see [`PROBATION_KEEP_FRAMES`]).
    pub(super) fn insert(
        &mut self,
        key: TextShapeKey,
        buffer: Buffer,
        extent: CachedExtent,
        left: f32,
    ) {
        // Counted here rather than per `shape_until_scroll` so one
        // cached run is one tally: the truncation back-off can reshape a
        // prefix several times to land inside the committed width, and a
        // workload test cares that the run was shaped, not how many
        // attempts the cut took. The memoized ellipsis probe shapes
        // without inserting and is deliberately not counted.
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
    /// Read by [`Self::insert`] and by [`Self::supersede`], the two sites
    /// that file one.
    fn probation_dies_at(&self) -> u64 {
        self.frame + PROBATION_KEEP_FRAMES + 1
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
    pub(super) fn supersede(&mut self, key: TextShapeKey) {
        let dies_at = self.probation_dies_at();
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        self.counters.supersedes.bump();
        // Never *extends* a life: an entry already closer to expiry —
        // one that was inserted and never looked up — keeps its own
        // deadline.
        if entry.dies_at > dies_at {
            entry.dies_at = dies_at;
            // The new ticket is earlier than the outstanding one, so it
            // is the one that decides this entry's fate: stamping it
            // here retires the supplanted ticket when it fires.
            entry.ticket_seq = self.expiry.schedule(key, dies_at);
        }
    }

    /// Advance the shared frame clock one frame and drop every buffer
    /// whose deadline has passed.
    ///
    /// **The one place the clock moves.** Everything downstream — the
    /// glyph atlas, the encoded-run cache — receives the new reading as
    /// an argument and never advances it, which is what keeps every text
    /// cache in the crate on one reading.
    ///
    /// Cost tracks what expires, not what is resident: [`Self::expiry`]
    /// hands back only the keys whose ticket came due, so a frame holding
    /// a scrolled document's whole working set pays the same as an empty
    /// one unless something actually lapsed.
    ///
    /// A ticket is a hint, never authority to drop. Deadlines move after
    /// it is filed — [`Self::hit`] pushes one out and deliberately files
    /// nothing, which is what keeps a re-read entry from filing a ticket
    /// per frame — so the real `dies_at` is re-read here and a still-live
    /// entry is simply re-filed.
    pub(super) fn tick_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        let entries = &mut self.entries;
        let recycle_pool = &mut self.recycle_pool;
        let counters = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already — a demote leaves two tickets outstanding
            // and both can come due in one drain, so whichever settled
            // first may have evicted the entry this one is holding.
            let Entry::Occupied(slot) = entries.entry(key) else {
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
            if slot.get().dies_at > frame {
                // Re-filed under the same serial, so the entry's stamp
                // still names it and nothing has to be written back.
                return Some(slot.get().dies_at);
            }
            counters.expiries.bump();
            recycle_into(recycle_pool, slot.remove().buffer);
            None
        });
    }

    /// Drop every shaped buffer now, recycling each one, without waiting
    /// out a retention window.
    ///
    /// `CosmicMeasure::load_font` owes this: the buffers were laid out
    /// against a database that has since changed. Tests that exercise the
    /// *restore* path use it to set up a guaranteed-cold cache in one
    /// call, instead of encoding this cache's retention policy into tests
    /// that aren't about it.
    pub(super) fn drop_all(&mut self) {
        let recycle_pool = &mut self.recycle_pool;
        for (_, entry) in self.entries.drain() {
            recycle_into(recycle_pool, entry.buffer);
        }
        self.expiry.clear();
    }

    /// A buffer to reshape into, or `None` when the caller has to build
    /// one. The pool's whole purpose: `Buffer::set_text` reclaims the
    /// line, shaping and layout allocations a recycled buffer already
    /// holds, where a fresh one pays for them again.
    pub(super) fn take_recycled(&mut self) -> Option<Buffer> {
        self.recycle_pool.pop()
    }

    /// Hand a buffer back that never became an entry — the ellipsis
    /// probe shapes one glyph and drops it. Filed buffers come back
    /// through [`Self::tick_frame`] and [`Self::drop_all`] instead.
    pub(super) fn recycle(&mut self, buffer: Buffer) {
        recycle_into(&mut self.recycle_pool, buffer);
    }
}

/// The pool's one write, as a free function: [`ShapedBufferCache::tick_frame`]
/// and [`ShapedBufferCache::drop_all`] both hold another field across it,
/// and only a field-level borrow leaves the pool free.
fn recycle_into(pool: &mut Vec<Buffer>, buffer: Buffer) {
    if pool.len() < RECYCLE_POOL_CAP {
        pool.push(buffer);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::common::counters::CounterSet;
    use crate::text::cosmic::counters::CacheCounts;

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    impl ShapedBufferCache {
        /// Outstanding expiry tickets. The number that says whether
        /// [`ShapedBufferCache::supersede`] is holding up its end of the
        /// wheel's protocol: a demote files a ticket that supplants the
        /// outstanding one, and if the supplanted ticket re-files itself
        /// this grows by one per demote for as long as the entry lives.
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        pub(crate) fn counts(&self) -> CacheCounts {
            self.counters.counts()
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
