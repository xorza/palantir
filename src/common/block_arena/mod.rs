//! Size-classed block arena for the cross-frame span stores: freed spans are taken again in place, so nothing is relocated and no frame pays for another's churn.
//!
//! An append-only store that compacts once dead exceeds live copies every live entry in one frame, on a fixed period under churn: a 120x frame once every 122 on `ChurnBench`, and a 239 µs worst frame against a 108 µs median on `damage/workload/shape_churn_partial` (137 µs without it, median unmoved). Uniform per-frame cost beats a lower average, and recycling removes the copy: no owner's [`Span`] is rewritten.
//!
//! A block is rounded up to a multiple of `GRANULE` and reusable only within its class, so [`BlockArena::release`] recovers capacity from the span length alone. The cost: **the arena is bounded by the sum over classes of each class's peak block count, not the peak live set**; span lengths drifting upward strand a block per class, so keep the longest span bounded (`drifting_run_lengths_strand_a_block_in_every_class_they_leave`).

use crate::common::counters::counter_snapshot;
use crate::common::non_max::NonMaxU32;
use crate::common::span::Span;

/// Size class of a span of `len` entries; `len` must be non-zero.
#[inline]
fn block_class<const GRANULE: u32>(len: u32) -> usize {
    const { assert!(GRANULE > 0, "a block holds at least one slot") };
    debug_assert!(len > 0, "an empty span is not allocated");
    ((len - 1) / GRANULE) as usize
}

#[inline]
const fn block_capacity<const GRANULE: u32>(class: usize) -> u32 {
    (class as u32 + 1) * GRANULE
}

/// Where the link of the block at `start` lives in `free_links`.
#[inline]
const fn link_idx<const GRANULE: u32>(start: u32) -> usize {
    (start / GRANULE) as usize
}

/// Flat storage carved into size-classed blocks, each freed block returned to its class's free list. Owners hold only a [`Span`]; allocation-free after warm-up.
///
/// A free list's links live beside the slots, at 4 bytes per granule. Refused: the link inside a free block's first slot, as [`slab`](https://docs.rs/slab) keeps it in a `Vacant` variant, which an arena that hands out `&[T]` cannot have, and which made each element carry a `u32` in a field of another meaning, a row's content hash among them.
///
/// `GRANULE` is the slot granularity: a span rounds up to a multiple of it. Coarse granules waste storage and cache density; fine ones mint a class per length, sharpening the drift bound. Short spans measured 3-7% slower coarse on `damage/workload/shape_churn_*`. One means exact fit.
#[derive(Debug)]
pub(crate) struct BlockArena<T, const GRANULE: u32> {
    pub(crate) slots: Vec<T>,
    /// Head of each class's free list, `None` when empty. LIFO, so the most recently freed (likely cached) block is reused.
    free_heads: Vec<Option<NonMaxU32>>,
    /// One per granule of [`Self::slots`]: the free block after the one that starts there. Read only for a free block.
    free_links: Vec<Option<NonMaxU32>>,
    pub(crate) counters: BlockArenaCounters,
}

impl<T, const GRANULE: u32> Default for BlockArena<T, GRANULE> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_heads: Vec::new(),
            free_links: Vec::new(),
            counters: BlockArenaCounters::default(),
        }
    }
}

impl<T: Copy, const GRANULE: u32> BlockArena<T, GRANULE> {
    /// Copies `items` into a block of their size class and returns the span. An empty `items` owns no block and never reaches the allocator.
    pub(crate) fn store(&mut self, items: &[T]) -> Span {
        let len = items.len() as u32;
        if len == 0 {
            return Span::new(0, 0);
        }
        let start = self.alloc_block(len, items[0]);
        self.slots[start as usize..start as usize + items.len()].copy_from_slice(items);
        Span::new(start, len)
    }

    /// Hands `span`'s block back to its class, recovered from `span.len` via [`block_class`].
    ///
    /// **The caller must not use `span` again.** A second release while the block is free links it to itself, so every later store of the class would get that one block; debug builds panic here, at the cost of a walk of the class's free list. A stale release after another owner took the block looks like a first release and is not caught.
    pub(crate) fn release(&mut self, span: Span) {
        if span.len == 0 {
            return;
        }
        let class = block_class::<GRANULE>(span.len);
        debug_assert!(
            class < self.free_heads.len(),
            "a live span's size class must already exist — it was allocated from",
        );
        debug_assert!(
            span.start.is_multiple_of(GRANULE),
            "a block starts on a granule",
        );
        debug_assert!(
            !self.is_free(span.start, class),
            "a block released twice",
        );
        self.free_links[link_idx::<GRANULE>(span.start)] = self.free_heads[class];
        self.free_heads[class] = Some(NonMaxU32::index(span.start as usize));
    }

    /// Whether the block at `start` is on `class`'s free list. Ends at the list's end, as a list has no cycle while [`Self::release`] refuses a free block.
    fn is_free(&self, start: u32, class: usize) -> bool {
        let mut at = self.free_heads[class];
        while let Some(block) = at {
            if block.get() == start {
                return true;
            }
            at = self.free_links[link_idx::<GRANULE>(block.get())];
        }
        false
    }

    /// Drops every block, live and free, keeping capacity.
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.free_heads.clear();
        self.free_links.clear();
    }

    /// Reserves a block for `len` entries, reusing a freed block of the class or extending storage; quiet [`BlockArenaCounters::allocs`] means the working set is saturated.
    ///
    /// `filler` fills a new block's slots until the caller writes them, and stays in those past its span, which nothing reads.
    fn alloc_block(&mut self, len: u32, filler: T) -> u32 {
        let class = block_class::<GRANULE>(len);
        if self.free_heads.len() <= class {
            self.free_heads.resize(class + 1, None);
        }
        if let Some(head) = self.free_heads[class] {
            self.free_heads[class] = self.free_links[link_idx::<GRANULE>(head.get())];
            self.counters.reuses.bump();
            return head.get();
        }
        self.counters.allocs.bump();
        let start = self.slots.len() as u32;
        self.slots.resize(
            start as usize + block_capacity::<GRANULE>(class) as usize,
            filler,
        );
        self.free_links
            .resize(self.free_links.len() + class + 1, None);
        debug_assert_eq!(
            self.free_links.len() * GRANULE as usize,
            self.slots.len(),
            "one link per granule",
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

#[cfg(test)]
pub(crate) mod internals {
    use super::*;

    impl<T, const GRANULE: u32> BlockArena<T, GRANULE> {
        /// Size classes currently holding a free block; a count climbing with uptime means drifting lengths.
        pub(crate) fn classes_with_free_blocks(&self) -> usize {
            self.free_heads.iter().flatten().count()
        }
    }
}

#[cfg(test)]
mod tests;
