//! Size-classed block arena for the cross-frame span stores: freed spans are taken again in place, so nothing is relocated and no frame pays for another's churn.
//!
//! An append-only store that compacts once dead exceeds live copies every live entry in one frame, on a fixed period under churn: a 120x frame once every 122 on `ChurnBench`, and a 239 µs worst frame against a 108 µs median on `damage/workload/shape_churn_partial` (137 µs without it, median unmoved). Uniform per-frame cost beats a lower average, and recycling removes the copy: no owner's [`Span`] is rewritten.
//!
//! A block is rounded up to a multiple of [`BlockSlot::GRANULE`] and reusable only within its class, so [`BlockArena::release`] recovers capacity from the span length alone. The cost: **the arena is bounded by the sum over classes of each class's peak block count, not the peak live set**; span lengths drifting upward strand a block per class, so keep the longest span bounded (`drifting_run_lengths_strand_a_block_in_every_class_they_leave`).

use crate::common::counters::counter_snapshot;
use crate::common::span::Span;

/// End of a size class's free list; a start is an index bounded by the owner population, so never `u32::MAX`. The gradient atlas's `MruList` has a same-valued sentinel, kept separate since each is sound by its own population bound.
const NIL: u32 = u32::MAX;

/// Size class of a span of `len` entries; `len` must be non-zero.
#[inline]
fn block_class<T: BlockSlot>(len: u32) -> usize {
    debug_assert!(len > 0, "an empty span is not allocated");
    ((len - 1) / T::GRANULE) as usize
}

#[inline]
const fn block_capacity<T: BlockSlot>(class: usize) -> u32 {
    (class as u32 + 1) * T::GRANULE
}

/// What an arena element owes so a free block can chain through it.
///
/// The free list is **intrusive**: a free block's first slot holds the link to the next of its class. Sound because **a free block's contents are never read**: reads go through a live owner's [`Span`], live until passed to [`BlockArena::release`].
pub(crate) trait BlockSlot: Copy {
    /// Slot granularity: a span rounds up to a multiple of it. Coarse granules waste storage and cache density; fine ones mint a class per length, sharpening the drift bound. Short spans measured 3-7% slower coarse on `damage/workload/shape_churn_*`. One means exact fit.
    const GRANULE: u32;

    fn free_link(next: u32) -> Self;

    fn next_free(self) -> u32;
}

/// Flat storage carved into size-classed blocks, each freed block returned to its class's intrusive free list. Owners hold only a [`Span`]; allocation-free after warm-up.
#[derive(Debug)]
pub(crate) struct BlockArena<T> {
    pub(crate) slots: Vec<T>,
    /// Head of each class's free list, [`NIL`] when empty. LIFO, so the most recently freed (likely cached) block is reused.
    free_heads: Vec<u32>,
    pub(crate) counters: BlockArenaCounters,
}

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
    /// Copies `items` into a block of their size class and returns the span. An empty `items` owns no block and never reaches the allocator.
    pub(crate) fn store(&mut self, items: &[T]) -> Span {
        let len = items.len() as u32;
        if len == 0 {
            return Span::new(0, 0);
        }
        let start = self.alloc_block(len);
        self.slots[start as usize..start as usize + items.len()].copy_from_slice(items);
        Span::new(start, len)
    }

    /// Hands `span`'s block back to its class, recovered from `span.len` via [`block_class`].
    ///
    /// **The caller must not use `span` again:** a double release links the block twice and the next two stores get the same block.
    pub(crate) fn release(&mut self, span: Span) {
        if span.len == 0 {
            return;
        }
        let class = block_class::<T>(span.len);
        debug_assert!(
            class < self.free_heads.len(),
            "a live span's size class must already exist — it was allocated from",
        );
        self.slots[span.start as usize] = T::free_link(self.free_heads[class]);
        self.free_heads[class] = span.start;
    }

    /// Drops every block, live and free, keeping capacity.
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.free_heads.clear();
    }

    /// Reserves a block for `len` entries, reusing a freed block of the class or extending storage; quiet [`BlockArenaCounters::allocs`] means the working set is saturated.
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
        self.slots.resize(
            start as usize + block_capacity::<T>(class) as usize,
            T::free_link(NIL),
        );
        start
    }
}

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    pub(crate) struct BlockArenaCounters;

    pub(crate) struct BlockArenaCounts;

    /// Spans that took a recycled block; `reuses` climbing while `allocs` stays flat means the working set is reached.
    reuses: u32,
    /// Spans that extended the arena for lack of a free block; if it never settles during warm-up, lengths keep drifting across classes.
    allocs: u32,
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use super::*;

    impl<T> BlockArena<T> {
        /// Size classes currently holding a free block; a count climbing with uptime means drifting lengths.
        pub(crate) fn classes_with_free_blocks(&self) -> usize {
            self.free_heads.iter().filter(|&&head| head != NIL).count()
        }
    }
}

#[cfg(test)]
mod tests;
