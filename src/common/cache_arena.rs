//! Bookkeeping primitive shared by `MeasureCache`, `EncodeCache`, and
//! `ComposeCache`: a `Vec<T>` arena paired with a `live: usize`
//! element count, plus the compaction-trigger heuristics. Each cache
//! holds one `LiveArena<T>` per independently-tracked element type and
//! coordinates them at the snapshot level (the per-snapshot type and
//! the in-place rewrite work stay cache-specific).
//!
//! Multiple parallel arenas of identical length share a single live
//! counter (e.g. encode-cache `starts` rides on `kinds.live`,
//! measure-cache `text` and `available` ride on `desired.live`).

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
    /// Mark `len` items previously owned by some snapshot as garbage.
    /// The `items` vec is unchanged — the slack lives until the next
    /// `compact`.
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
