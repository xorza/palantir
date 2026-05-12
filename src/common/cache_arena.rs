//! Bookkeeping primitive used by `MeasureCache`: a `Vec<T>` arena
//! paired with a `live: usize` element count, plus the
//! compaction-trigger heuristics. The cache holds one `LiveArena<T>`
//! per independently-tracked element type and coordinates them at
//! the snapshot level (the per-snapshot type and the in-place rewrite
//! work stay cache-specific).
//!
//! Multiple parallel arenas of identical length share a single live
//! counter (e.g. measure-cache `text` and `available` ride on
//! `desired.live`).

/// Compact when arena length exceeds `live × COMPACT_RATIO` — i.e. at
/// least half the arena is garbage.
pub(crate) const COMPACT_RATIO: usize = 2;
/// Floor below which compaction never triggers — small caches don't
/// repay the rebuild cost.
pub(crate) const COMPACT_FLOOR: usize = 64;

pub(crate) struct LiveArena<T> {
    pub(crate) items: Vec<T>,
    pub(crate) live: usize,
}

// Manual `Default` so `LiveArena<CmdKind>` works without `CmdKind: Default`.
impl<T> Default for LiveArena<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            live: 0,
        }
    }
}

impl<T> LiveArena<T> {
    /// Account for `len` items just appended to `items` and now owned
    /// by a snapshot. Caller has already extended `items`; this only
    /// updates the live counter. Asserts the post-condition `live <=
    /// items.len()` — catches a missing `extend_from_slice` before
    /// the inconsistency reaches `release` or `compact`.
    pub(crate) fn acquire(&mut self, len: u32) {
        self.live += len as usize;
        assert!(self.live <= self.items.len());
    }

    /// Mark `len` items previously owned by some snapshot as garbage.
    /// The `items` vec is unchanged — the slack lives until the next
    /// `compact`. Asserts in release: a double-release (or releasing
    /// more than was acquired) would silently underflow `live` and
    /// poison both the compaction trigger and `compact`'s capacity
    /// sizing — worth panicking immediately.
    pub(crate) fn release(&mut self, len: u32) {
        assert!(self.live >= len as usize);
        self.live -= len as usize;
    }

    /// At least half the arena is garbage AND the arena holds enough
    /// live items for the rebuild to pay for itself. Caches with
    /// multiple independent arenas should compact when ANY of them
    /// reports `true` — the per-arena form keeps a tiny-but-overgrown
    /// arena from triggering on a co-resident large arena's account.
    pub(crate) fn needs_compact(&self) -> bool {
        self.items.len() > self.live.saturating_mul(COMPACT_RATIO) && self.live > COMPACT_FLOOR
    }

    /// Reachable only from the cache `clear()` methods, themselves
    /// reachable only from `internals` (tests + `internals`
    /// feature). Same gate so the production build sees no dead code.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.live = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_decrements_live_without_touching_items() {
        let mut a: LiveArena<u32> = LiveArena::default();
        a.items.extend_from_slice(&[1, 2, 3]);
        a.acquire(3);
        assert_eq!(a.live, 3, "acquire bumped live");
        a.release(2);
        assert_eq!(a.live, 1);
        assert_eq!(a.items.len(), 3, "release leaves items as garbage");
    }

    #[test]
    #[should_panic(expected = "assertion failed")]
    fn acquire_past_items_len_panics() {
        // Forgot the `extend_from_slice` before `acquire` — the
        // post-condition assert must trip immediately rather than let
        // the drift reach `compact` or `release`.
        let mut a: LiveArena<u32> = LiveArena::default();
        a.acquire(1);
    }

    #[test]
    #[should_panic(expected = "assertion failed")]
    fn release_underflow_panics() {
        let mut a: LiveArena<u32> = LiveArena::default();
        a.items.extend_from_slice(&[1]);
        a.acquire(1);
        a.release(2);
    }

    #[test]
    fn needs_compact_predicate_cases() {
        let live_above = COMPACT_FLOOR + 10;
        // (label, items_len, live, expected)
        let cases: &[(&str, usize, usize, bool)] = &[
            ("below_floor_heavy_garbage", 10_000, COMPACT_FLOOR, false),
            (
                "ratio_boundary_not_crossed",
                live_above * COMPACT_RATIO,
                live_above,
                false,
            ),
            (
                "both_arms_cross",
                live_above * COMPACT_RATIO + 1,
                live_above,
                true,
            ),
        ];
        for (label, items_len, live, expected) in cases {
            let mut a: LiveArena<u32> = LiveArena::default();
            a.items.resize(*items_len, 0);
            a.live = *live;
            assert_eq!(a.needs_compact(), *expected, "case: {label}");
        }
    }

    #[test]
    fn clear_resets_both_arms() {
        let mut a: LiveArena<u32> = LiveArena::default();
        a.items.extend_from_slice(&[1, 2]);
        a.acquire(2);
        a.clear();
        assert_eq!(a.live, 0);
        assert!(a.items.is_empty());
    }
}
