//! Block allocation, recycling and class isolation.

use super::*;
use crate::common::counters::CounterSet;

/// The cheapest thing satisfying the trait: the link is the whole
/// element, so a test can read a slot back and say what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot(u32);

impl BlockSlot for Slot {
    /// The coarse setting, so these cases pin the rounding rather
    /// than the exact-fit degenerate case; `Paint`'s granule of one
    /// is covered by the damage tests it was measured for.
    const GRANULE: u32 = 4;

    fn free_link(next: u32) -> Self {
        Self(next)
    }

    fn next_free(self) -> u32 {
        self.0
    }
}

fn store(arena: &mut BlockArena<Slot>, tags: impl Iterator<Item = u32>) -> Span {
    let items: Vec<Slot> = tags.map(Slot).collect();
    arena.store(&items)
}

/// Hand-computed class boundaries. This fixture's granule is 4, so a class
/// covers four lengths and its capacity is the top of that range —
/// the relationship `release` recovers a block's size from.
#[test]
fn size_classes_round_up_to_the_granule() {
    for (len, class) in [(1u32, 0usize), (4, 0), (5, 1), (8, 1), (9, 2), (40, 9)] {
        assert_eq!(block_class::<Slot>(len), class, "len {len}");
        assert!(
            block_capacity::<Slot>(class) >= len,
            "class {class} must hold the length that chose it",
        );
    }
    for class in [0usize, 1, 2, 9] {
        assert_eq!(block_capacity::<Slot>(class), 4 * (class as u32 + 1));
        assert_eq!(
            block_class::<Slot>(block_capacity::<Slot>(class)),
            class,
            "a class's own capacity must map back to it",
        );
    }
}

/// A released block is handed straight back to the next span of its
/// class, so a workload that keeps re-storing the same length stops
/// growing the arena after the first store.
#[test]
fn a_released_block_is_the_next_one_handed_out() {
    let mut arena = BlockArena::<Slot>::default();
    let first = store(&mut arena, 0..3);
    assert_eq!((first.start, first.len), (0, 3));
    assert_eq!(arena.slots.len(), 4, "3 entries round up to a 4-slot block");

    for round in 1u32..=5 {
        arena.release(first);
        let again = store(&mut arena, round * 10..round * 10 + 3);
        assert_eq!(again, first, "the same block comes back every time");
        assert_eq!(arena.slots.len(), 4, "round {round} must not extend");
        assert_eq!(
            &arena.slots[again.range()],
            &[Slot(round * 10), Slot(round * 10 + 1), Slot(round * 10 + 2)],
            "and it holds this round's entries, not the last one's",
        );
    }
    let counts = arena.counters.counts();
    assert_eq!((counts.allocs, counts.reuses), (1, 5));
}

/// Blocks are per class and never split or coalesced: a span only
/// ever takes a block its own class freed, whatever is parked
/// elsewhere.
#[test]
fn a_block_is_only_reused_within_its_own_class() {
    let mut arena = BlockArena::<Slot>::default();
    // 2 → class 0 (4 slots), 5 → class 1 (8), 9 → class 2 (12).
    let small = store(&mut arena, 0..2);
    let medium = store(&mut arena, 100..105);
    let large = store(&mut arena, 200..209);
    assert_eq!((small.start, medium.start, large.start), (0, 4, 12));
    assert_eq!(arena.slots.len(), 24);

    // Free the two big ones; a class-0 span still cannot touch them.
    arena.release(medium);
    arena.release(large);
    let another_small = store(&mut arena, 300..302);
    assert_eq!(
        another_small.start, 24,
        "class 0 was empty, so this extends rather than raiding class 1 or 2",
    );
    assert_eq!(arena.slots.len(), 28);

    // Their own classes do reclaim them, newest-freed first within a
    // class — LIFO, so `large` is not what a class-1 span gets.
    assert_eq!(store(&mut arena, 400..405).start, medium.start);
    assert_eq!(store(&mut arena, 500..509).start, large.start);
    assert_eq!(arena.slots.len(), 28, "both came off free lists");

    let counts = arena.counters.counts();
    assert_eq!((counts.allocs, counts.reuses), (4, 2));
}

/// LIFO within a class, which is the whole reason the free list is a
/// chain rather than a queue: the block handed back is the one most
/// recently released and therefore the one most likely still in cache.
#[test]
fn a_class_hands_back_the_most_recently_freed_block() {
    let mut arena = BlockArena::<Slot>::default();
    let spans: Vec<Span> = (0..3).map(|i| store(&mut arena, i..i + 3)).collect();
    for span in &spans {
        arena.release(*span);
    }
    // Released 0, 1, 2 — so they come back 2, 1, 0.
    for want in spans.iter().rev() {
        assert_eq!(store(&mut arena, 0..3), *want);
    }
    assert_eq!(arena.slots.len(), 12, "three blocks, all recycled");
}

/// A shorter span reusing a longer one's block must expose only the
/// entries it wrote — the slack past its length belongs to nobody.
#[test]
fn a_shorter_span_sees_only_its_own_entries() {
    let mut arena = BlockArena::<Slot>::default();
    let four = store(&mut arena, 10..14);
    arena.release(four);
    // 2 and 4 share class 0, so this takes the 4-slot block.
    let two = store(&mut arena, 90..92);
    assert_eq!(two.start, four.start);
    assert_eq!(two.len, 2);
    assert_eq!(&arena.slots[two.range()], &[Slot(90), Slot(91)]);
}

/// An empty store owns no block, so it neither reaches the allocator
/// nor corrupts a class when it is released.
#[test]
fn an_empty_span_owns_no_block() {
    let mut arena = BlockArena::<Slot>::default();
    let empty = arena.store(&[]);
    assert_eq!((empty.start, empty.len), (0, 0));
    assert_eq!(arena.slots.len(), 0);
    arena.release(empty);
    assert_eq!(arena.counters.counts().allocs, 0);

    // And a real store after it still starts at 0.
    assert_eq!(store(&mut arena, 0..2).start, 0);
}

/// `clear` drops the free lists with the storage. Keeping them would
/// leave every head pointing into a buffer that no longer holds
/// blocks, and the next store would hand out an index into somebody
/// else's entries.
#[test]
fn clear_drops_the_free_lists_with_the_storage() {
    let mut arena = BlockArena::<Slot>::default();
    let span = store(&mut arena, 0..3);
    arena.release(span);
    arena.clear();
    assert_eq!(arena.slots.len(), 0);

    let fresh = store(&mut arena, 7..10);
    assert_eq!((fresh.start, fresh.len), (0, 3));
    assert_eq!(arena.slots.len(), 4, "extended, not taken off a stale head");
    assert_eq!(&arena.slots[fresh.range()], &[Slot(7), Slot(8), Slot(9)]);
}
