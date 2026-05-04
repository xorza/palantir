//! Bookkeeping primitive shared by `EncodeCache` and `ComposeCache`: a
//! `Vec<T>` arena paired with a `live: usize` byte count, plus the
//! compaction-trigger heuristics. Each cache holds one `LiveArena<T>`
//! per element type and coordinates them at the snapshot level (the
//! per-snapshot type and the in-place rewrite work stay cache-specific).

/// Compact when arena length exceeds `live × COMPACT_RATIO` — i.e. at
/// least half the arena is garbage. Tuned in lockstep with
/// `MeasureCache`; revisit there before changing here.
const COMPACT_RATIO: usize = 2;
/// Floor below which compaction never triggers — small caches don't
/// repay the rebuild cost.
const COMPACT_FLOOR: usize = 64;

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
    /// Mark `len` items previously owned by some snapshot as garbage.
    /// The `items` vec is unchanged — the slack lives until the next
    /// `compact`.
    pub(crate) fn release(&mut self, len: u32) {
        assert!(self.live >= len as usize);
        self.live -= len as usize;
    }

    /// At least half the arena is garbage.
    pub(crate) fn is_overgrown(&self) -> bool {
        self.items.len() > self.live.saturating_mul(COMPACT_RATIO)
    }

    /// Arena is large enough for compaction to be worth the rebuild.
    pub(crate) fn over_floor(&self) -> bool {
        self.live > COMPACT_FLOOR
    }

    /// Reachable only from the cache `clear()` methods, themselves
    /// reachable only from `bench_support` (tests + `bench-support`
    /// feature). Same gate so the production build sees no dead code.
    #[cfg(any(test, feature = "bench-support"))]
    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.live = 0;
    }
}
