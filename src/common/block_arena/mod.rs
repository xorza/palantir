//! Size-classed block arena for the cross-frame span stores: spans are
//! handed back and taken again in place, so nothing is ever relocated
//! and no frame pays for another frame's churn.
//!
//! # Why not an append-only arena
//!
//! The obvious store appends every write to the tail and leaves the
//! replaced span behind as dead space, compacting once dead exceeds
//! live. Compaction copies *every live entry* in a single frame, and
//! under churn — where each frame appends one frame's worth and retires
//! one frame's worth, so live stays flat — the trigger fires on a fixed
//! period. Both tenants arrived here from that, measuring the same
//! shape:
//!
//! - **Encoded runs.** Isolated on `ChurnBench`, the compaction frame
//!   against the median frame: 0.7 µs against 19 at 8 × 12 glyphs,
//!   3.0 against 271 at 50 × 25, and 21 against 2520 at 200 × 40 — a
//!   120x frame, once every 122.
//! - **Paint snapshots.** Whole frames on `damage/workload/
//!   shape_churn_partial`, which compacted once every ~924: the median
//!   frame ran 108 µs and the worst 239. Removing it took that worst
//!   frame to 137 µs (−43%) and left the median, p75, p90 and p99
//!   unmoved inside noise — the trade this is, stated as measured.
//!
//! Amortised that is free — the copy per frame averages exactly one
//! frame's appends — but a doubled frame arriving unannounced is a
//! dropped one, and "uniform per-frame cost is worth more than a lower
//! average" is the rule this crate's sweeps already state. Recycling
//! blocks in place removes the copy entirely instead of spreading it:
//! nothing is relocated, so no owner's [`Span`] is ever rewritten and no
//! owner needs to be reachable at reclaim time.
//!
//! # What the size classes buy, and what they cost
//!
//! A block's storage is rounded up to a multiple of the tenant's
//! [`BlockSlot::GRANULE`], and a freed block is reusable only by a span
//! in the same class — which is what lets it be handed back and taken
//! again without ever moving anything, and what lets
//! [`BlockArena::release`] recover a block's capacity from the span
//! length alone rather than from a per-owner field.
//!
//! The cost is that **the arena is bounded by the sum over classes of
//! each class's peak block count, not by the peak live set**. A workload
//! whose span lengths drift monotonically upward strands a block in every
//! class it leaves behind, and nothing brings those back except a length
//! returning to that class. Growth is then quadratic in the longest span
//! ever stored and linear in the owner count, which is why the number that
//! has to stay bounded is the longest span — see
//! `drifting_run_lengths_strand_a_block_in_every_class_they_leave`.

use crate::common::counters::counter_snapshot;
use crate::primitives::span::Span;

/// End of a size class's free list. Distinguishable from every real
/// block start: a start is an index into the arena, which is bounded by
/// the owner population, and `u32::MAX` slots is a buffer no tenant can
/// reach.
///
/// The gradient atlas's `MruList` carries a sentinel of the same value
/// for the same reason, and the two stay apart deliberately: they index
/// unrelated spaces, and each is only sound because of a bound argued
/// against *its* population. Sharing the constant would make the two
/// arguments look like one.
const NIL: u32 = u32::MAX;

/// Size class of a span of `len` entries. `len` must be non-zero — an
/// empty span owns no block and never reaches the allocator.
#[inline]
fn block_class<T: BlockSlot>(len: u32) -> usize {
    debug_assert!(len > 0, "an empty span is not allocated");
    ((len - 1) / T::GRANULE) as usize
}

/// Slots a block of `class` holds. The inverse of [`block_class`] on the
/// class boundary, which is what makes [`BlockArena::release`] able to
/// recover a block's capacity from the span length alone.
#[inline]
fn block_capacity<T: BlockSlot>(class: usize) -> u32 {
    (class as u32 + 1) * T::GRANULE
}

/// What an arena element owes so a free block can chain through it.
///
/// The free list is **intrusive**: a block's first slot holds the link to
/// the next free block of its class, so the per-class index is a single
/// `u32` head rather than a `Vec` per class — no second allocation, no
/// pointer chase off to the side, and the link lands in the very cache
/// line [`BlockArena::store`] is about to write.
///
/// That is sound only because **a free block's contents are never read**.
/// Every read goes through a live owner's [`Span`], and an owner's span
/// is live exactly until it is passed to [`BlockArena::release`]. An
/// implementor may therefore overwrite any field it likes; re-allocation
/// overwrites them all.
pub(crate) trait BlockSlot: Copy {
    /// Slot granularity of this tenant's blocks: a span's storage rounds
    /// up to a multiple of it, and only a span in the same class can
    /// take a freed block.
    ///
    /// **The right value follows from how long this tenant's spans are**,
    /// and the two here differ by 4x for that reason. Rounding trades
    /// slack for fewer classes: coarse granules waste storage and the
    /// cache density that comes with it, fine ones recycle exactly but
    /// mint a class per distinct length, which sharpens the drift bound
    /// below. Long spans can afford the slack because it is a small
    /// fraction of them; short ones cannot, and were measured at 3-7%
    /// on `damage/workload/shape_churn_*` when they paid it.
    ///
    /// One, meaning exact fit, is a legitimate setting — the free lists
    /// then key on the exact length and nothing is wasted.
    const GRANULE: u32;

    /// A free block whose next sibling in its class starts at `next`, or
    /// [`NIL`] when it is the last.
    fn free_link(next: u32) -> Self;

    /// The link back out — the value the matching [`Self::free_link`]
    /// stored.
    fn next_free(self) -> u32;
}

/// Flat storage carved into size-classed blocks, with each freed block
/// returned to its class's intrusive free list.
///
/// Owners hold a [`Span`] into [`Self::slots`] and nothing else: the
/// arena never walks them, never relocates a block, and never needs to
/// reach an owner to reclaim one. After warm-up it is allocation-free —
/// storage and free lists both retain capacity.
#[derive(Debug)]
pub(crate) struct BlockArena<T> {
    /// Block storage. Grows to the working set's high-water mark and is
    /// then reused in place, so a live span is stable for its whole life.
    pub(crate) slots: Vec<T>,
    /// Head of each size class's free list, [`NIL`] when the class is
    /// empty — `free_heads[c]` starts a chain of blocks of
    /// `block_capacity(c)` slots, linked through their first slot. LIFO,
    /// so the block handed out is the one most recently freed and
    /// therefore the one most likely still in cache.
    ///
    /// Flat by construction: one `u32` per class, and the chain itself
    /// costs nothing because it lives in space that is already free.
    free_heads: Vec<u32>,
    /// Allocation / recycling tallies. Zero-sized outside test builds.
    pub(crate) counters: BlockArenaCounters,
}

/// Hand-written rather than derived: the derive would demand
/// `T: Default`, which neither tenant's element type has a reason to be.
impl<T> Default for BlockArena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_heads: Vec::new(),
            counters: BlockArenaCounters::default(),
        }
    }
}

impl<T: BlockSlot> BlockArena<T> {
    /// Copy `items` into a block of their size class and answer the span
    /// covering them.
    ///
    /// An empty `items` is a legitimate store — an all-whitespace text
    /// run, a node that paints nothing — and owns no block, so it must
    /// not reach the allocator. The empty span it answers makes its later
    /// [`Self::release`] a no-op.
    pub(crate) fn store(&mut self, items: &[T]) -> Span {
        let len = items.len() as u32;
        if len == 0 {
            return Span::new(0, 0);
        }
        let start = self.alloc_block(len);
        self.slots[start as usize..start as usize + items.len()].copy_from_slice(items);
        Span::new(start, len)
    }

    /// Hand `span`'s block back to its size class.
    ///
    /// The class is recovered from `span.len` rather than stored: every
    /// block was allocated by [`Self::alloc_block`] for exactly this
    /// length, so [`block_class`] maps it back to the list it came from.
    ///
    /// **The caller must not use `span` again.** Nothing here can tell a
    /// double release from two spans that happen to be equal, and a
    /// double release links the block into its class twice — the next two
    /// stores would then be handed the same block.
    pub(crate) fn release(&mut self, span: Span) {
        if span.len == 0 {
            return;
        }
        let class = block_class::<T>(span.len);
        debug_assert!(
            class < self.free_heads.len(),
            "a live span's size class must already exist — it was allocated from",
        );
        // Push onto the class's chain: the block's first slot takes the
        // old head, and the block itself becomes the new one.
        self.slots[span.start as usize] = T::free_link(self.free_heads[class]);
        self.free_heads[class] = span.start;
    }

    /// Drop every block, live and free alike, keeping storage capacity.
    /// For an owner that just discarded every span it held — reclaiming
    /// them one at a time would be the same work with more chances to
    /// miss one.
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.free_heads.clear();
    }

    /// Reserve a block for `len` entries, reusing a freed block of the
    /// same size class when one is available and extending the storage
    /// when it is not.
    ///
    /// The extension is the only path that grows the arena, so
    /// [`BlockArenaCounters::allocs`] going quiet is exactly the statement
    /// "the working set is saturated and every span now recycles".
    fn alloc_block(&mut self, len: u32) -> u32 {
        let class = block_class::<T>(len);
        if self.free_heads.len() <= class {
            self.free_heads.resize(class + 1, NIL);
        }
        let head = self.free_heads[class];
        if head != NIL {
            self.free_heads[class] = self.slots[head as usize].next_free();
            self.counters.reuses.bump();
            return head;
        }
        self.counters.allocs.bump();
        let start = self.slots.len() as u32;
        // The slack past `len` is never read — a span covers only the
        // entries actually written — so the fill is whatever is cheapest
        // to name.
        self.slots.resize(
            start as usize + block_capacity::<T>(class) as usize,
            T::free_link(NIL),
        );
        start
    }
}

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    /// What the allocator did. See [`BlockArena::alloc_block`] for why
    /// these two are the health check on the whole scheme.
    pub(crate) struct BlockArenaCounters;

    /// One reading of a [`BlockArenaCounters`]. Subtract two to get what
    /// a span of frames did.
    pub(crate) struct BlockArenaCounts;

    /// Spans that took a recycled block off a free list.
    ///
    /// Paired with [`Self::allocs`] this is the whole health check on the
    /// scheme: recycling is what replaced compaction, and it only holds
    /// if a workload's spans keep landing in size classes their
    /// predecessors freed. `reuses` climbing while `allocs` stays flat is
    /// the statement "the arena has reached its working set and stopped
    /// growing".
    reuses: u32,
    /// Spans that had to extend the arena because their size class had no
    /// free block. Expected during warm-up and whenever a genuinely new
    /// length appears; a workload where this never settles is one whose
    /// lengths keep drifting across class boundaries, and its arena grows
    /// to the sum of every class's peak.
    allocs: u32,
}

/// Gated with its readers — this module's own tests and the damage
/// benchmark's arena-settle guard — rather than on `internals`, which
/// the two integration suites enable without ever asking this question.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use super::*;

    impl<T> BlockArena<T> {
        /// Size classes currently holding at least one free block.
        ///
        /// The number that says whether recycling is landing where it
        /// should: a workload storing one length wants exactly one class
        /// parked, and a count that climbs with uptime is a workload
        /// whose lengths are drifting across class boundaries.
        pub(crate) fn classes_with_free_blocks(&self) -> usize {
            self.free_heads.iter().filter(|&&head| head != NIL).count()
        }
    }
}

#[cfg(test)]
mod tests;
