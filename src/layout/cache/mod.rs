//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `src/layout/measure-cache.md`.
//!
//! **Storage**: SoA arenas — three node-indexed and parallel, bundled
//! into [`NodeArenas`] (`desired`, `text_spans`, `scroll_content`) so
//! length-equality is structural; plus two variable-length per-subtree
//! [`LiveArena`]s (`hugs` for grid descendants, `text_shapes_arena` for
//! `Shape::Text` runs) — plus a tiny per-`WidgetId` `ArenaSnapshot`
//! pointing at a contiguous range. The dimensional cache key
//! (quantized `available`) lives directly on `ArenaSnapshot` as a
//! per-snapshot scalar, not in a parallel arena. Steady-state writes
//! are in-place memcpys when the subtree size matches; size mismatches
//! fall back to append + mark-garbage with periodic compaction.
//!
//! `NodeArenas` owns one shared `live` counter for its three columns;
//! each variable-length `LiveArena` tracks its own. Compaction
//! constants live in `src/common/cache_arena.rs`.
//!
//! Compaction kicks in when an arena holds more than `live ×
//! COMPACT_RATIO` items. It walks every snapshot, rewrites their
//! `start` indices to point at a freshly-packed arena, and drops the
//! old one. O(live) — a one-frame cost paid infrequently.
//!
//! Eviction (via [`MeasureCache::sweep_removed`]) drops the snapshot
//! and releases its arena ranges; the slots stay as garbage until the
//! next compact.

use crate::common::cache_arena::{COMPACT_FLOOR, COMPACT_RATIO, LiveArena};
use crate::layout::result::ShapedText;
use crate::layout::types::span::Span;
use crate::primitives::size::Size;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::IVec2;
use rustc_hash::FxHashMap;
use std::ops::Range;

/// Snapshot index entry. `nodes` indexes the [`NodeArenas`] columns;
/// `hugs` indexes `hugs`; `text_shapes` indexes `text_shapes_arena`.
/// The snapshot key is `(subtree_hash, available_q)` — both stored
/// inline so the validity check on `try_lookup` doesn't have to
/// dereference a separate per-node arena.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub(crate) subtree_hash: NodeHash,
    /// Quantized `available` size at snapshot time — the dimensional
    /// half of the cache-validity check.
    pub(crate) available_q: AvailableKey,
    /// Range over [`NodeArenas`]. `nodes.desired[range()]` is the
    /// subtree's `desired` in pre-order; index 0 is the snapshot root's
    /// own size.
    pub(crate) nodes: Span,
    /// Range over `hugs`. Per-grid hug arrays for every
    /// `LayoutMode::Grid` descendant of the subtree, in pre-order.
    /// Each grid contributes four arrays in fixed order:
    /// cols.max, cols.min, rows.max, rows.min. `Span::EMPTY` for
    /// grid-free subtrees. Length stable across frames as long as
    /// `subtree_hash` is unchanged because the hash includes every
    /// descendant `GridDef` (track count + sizing).
    pub(crate) hugs: Span,
    /// Range over `text_shapes_arena`. Variable-length flat buffer of
    /// shaped text runs for every `Shape::Text` in the subtree, in
    /// pre-order. The `text_spans` slice (parallel to `desired`)
    /// stores **subtree-local** spans into this range.
    pub(crate) text_shapes: Span,
}

/// Quantized `available` size — the dimensional half of the cache
/// key. `i32::MAX` on either axis represents an infinite available
/// (ZStack / Hug parents propagate `f32::INFINITY`). Equality compare
/// is used as a cache-validity gate.
pub(crate) type AvailableKey = IVec2;

/// Per-subtree slice bundle: borrows into the three parallel
/// node-indexed arenas (`desired`, `text_spans`, `scroll_content`)
/// plus the per-grid `hugs` and the flat `text_shapes` payloads. The
/// three node-indexed slices share length and pre-order alignment;
/// `hugs` is sized per-grid descendant in `HUG_ORDER`; `text_shapes`
/// is sized per text-shape in pre-order.
///
/// `text_spans` carries spans whose `start` is offset by
/// `text_spans_base`. [`MeasureCache::write_subtree`] subtracts
/// `text_spans_base` per element so the stored form is subtree-local
/// (and thus survives compaction of the flat range).
/// [`MeasureCache::try_lookup`] returns spans already in subtree-local
/// form with `text_spans_base = 0`, so the caller can rebase by
/// adding its destination offset directly. Lets writers hand over
/// their global per-frame `text_spans` slice and a single offset
/// scalar — no scratch buffer required.
pub(crate) struct SubtreeArenas<'a> {
    pub(crate) desired: &'a [Size],
    /// Per-node `Span` into the flat `text_shapes` buffer. Spans are
    /// expressed in `text_spans_base`-rooted coordinates: subtract
    /// `text_spans_base` from each `start` to get a subtree-local
    /// index into [`Self::text_shapes`].
    pub(crate) text_spans: &'a [Span],
    /// Base offset to subtract from each `text_spans[i].start`. Zero
    /// on read (cache stores subtree-local already); non-zero on
    /// write (caller's per-frame `text_spans` slice indexes a global
    /// flat buffer, this offset rebases it).
    pub(crate) text_spans_base: u32,
    /// Per-node measured content extent for `LayoutMode::Scroll`
    /// descendants, `Size::ZERO` elsewhere.
    pub(crate) scroll_content: &'a [Size],
    /// Per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// of the subtree, packed in pre-order. Each grid contributes
    /// four arrays in fixed order — cols.max, cols.min, rows.max,
    /// rows.min — for `2 * (n_cols + n_rows)` floats per grid.
    /// Empty for grid-free subtrees.
    pub(crate) hugs: &'a [f32],
    /// Flat per-text-shape buffer, in pre-order over the subtree's
    /// `Shape::Text` runs. Empty for text-free subtrees.
    pub(crate) text_shapes: &'a [ShapedText],
}

/// What [`MeasureCache::try_lookup`] returns on a hit. `root` is
/// the snapshot root's own `desired` — the value `measure` returns
/// up the recursion. `arenas` carries the slices ready to
/// `copy_from_slice` into the caller's destination columns.
pub(crate) struct CachedSubtree<'a> {
    pub(crate) root: Size,
    pub(crate) arenas: SubtreeArenas<'a>,
}

#[inline]
fn quantize_axis(v: f32) -> i32 {
    if !v.is_finite() {
        i32::MAX
    } else {
        v.round() as i32
    }
}

#[inline]
pub(crate) fn quantize_available(s: Size) -> AvailableKey {
    // Layout invariants keep `available` in `[0, ∞)`; pin the contract
    // here so a future regression trips early.
    assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
    IVec2::new(quantize_axis(s.w), quantize_axis(s.h))
}

/// The three node-indexed parallel columns. Length-equality is
/// structural: every mutation goes through methods that touch all
/// three at once, so the columns can't drift. `live` counts elements
/// still referenced by a snapshot; the underlying `Vec`s carry that
/// plus garbage from released snapshots until the next compact.
#[derive(Default)]
pub(crate) struct NodeArenas {
    pub(crate) desired: Vec<Size>,
    /// Per-node `Span` into each snapshot's flat `text_shapes_arena`
    /// range. Stored **subtree-local** (start relative to the snapshot's
    /// `text_shapes` range start) so spans survive flat-range compaction.
    pub(crate) text_spans: Vec<Span>,
    /// Per-node measured content extent for `LayoutMode::Scroll`
    /// (zero elsewhere). Restored on a cache hit so the
    /// `LayoutResult.scroll_content` slice is populated without
    /// re-running the stack/zstack measure.
    pub(crate) scroll_content: Vec<Size>,
    pub(crate) live: usize,
}

impl NodeArenas {
    fn write_in_place(&mut self, range: Range<usize>, src: &SubtreeArenas<'_>) {
        self.desired[range.clone()].copy_from_slice(src.desired);
        let base = src.text_spans_base;
        for (dst, s) in self.text_spans[range.clone()]
            .iter_mut()
            .zip(src.text_spans.iter())
        {
            *dst = Span {
                start: s.start.saturating_sub(base),
                len: s.len,
            };
        }
        self.scroll_content[range].copy_from_slice(src.scroll_content);
    }

    fn append(&mut self, src: &SubtreeArenas<'_>) -> u32 {
        let start = self.desired.len() as u32;
        self.desired.extend_from_slice(src.desired);
        let base = src.text_spans_base;
        self.text_spans.extend(src.text_spans.iter().map(|s| Span {
            start: s.start.saturating_sub(base),
            len: s.len,
        }));
        self.scroll_content.extend_from_slice(src.scroll_content);
        start
    }

    fn extend_from_within(&mut self, src: &Self, range: Range<usize>) -> u32 {
        let start = self.desired.len() as u32;
        self.desired.extend_from_slice(&src.desired[range.clone()]);
        self.text_spans
            .extend_from_slice(&src.text_spans[range.clone()]);
        self.scroll_content
            .extend_from_slice(&src.scroll_content[range]);
        start
    }

    fn acquire(&mut self, len: u32) {
        self.live += len as usize;
        assert!(self.live <= self.desired.len());
    }

    fn release(&mut self, len: u32) {
        assert!(self.live >= len as usize);
        self.live -= len as usize;
    }

    fn needs_compact(&self) -> bool {
        self.desired.len() > self.live.saturating_mul(COMPACT_RATIO) && self.live > COMPACT_FLOOR
    }

    fn with_capacity(cap: usize) -> Self {
        Self {
            desired: Vec::with_capacity(cap),
            text_spans: Vec::with_capacity(cap),
            scroll_content: Vec::with_capacity(cap),
            live: 0,
        }
    }

    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.desired.clear();
        self.text_spans.clear();
        self.scroll_content.clear();
        self.live = 0;
    }
}

#[derive(Default)]
pub(crate) struct MeasureCache {
    pub(crate) nodes: NodeArenas,
    /// Per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// of every cached subtree, packed in pre-order. Snapshot
    /// records `(hugs_start, hugs_len)` into this arena. Lets a
    /// cache hit restore `LayoutEngine.scratch.grid.hugs` for the
    /// cached subtree's grids without walking children —
    /// `grid::arrange` then resolves track sizes correctly. Without
    /// this, a cache hit at any ancestor of a Grid would leave `hugs`
    /// zeroed and the grid would collapse every cell to (0, 0).
    pub(crate) hugs: LiveArena<f32>,
    /// Flat shaped-text buffer for every `Shape::Text` in every
    /// cached subtree, packed in pre-order. Snapshot records
    /// `(text_shapes_start, len)` into this arena. `text_spans`
    /// (per-node, subtree-local) addresses entries within each
    /// snapshot's range. Tracks its own live counter — text shapes
    /// don't appear on every node, so length doesn't ride on
    /// `desired.live`.
    pub(crate) text_shapes_arena: LiveArena<ShapedText>,
    /// Per-`WidgetId` snapshot index. Each value points at a range in
    /// the arenas above.
    pub(crate) snapshots: FxHashMap<WidgetId, ArenaSnapshot>,
}

impl MeasureCache {
    /// Validate the cache for `wid` against the current frame's
    /// `(subtree_hash, available_q)`. Both halves of the key live on
    /// the snapshot — no parallel-arena indirection. On hit, return a
    /// [`CachedSubtree`] with the root's `desired` and the arena
    /// slices ready to copy. On miss, `None`.
    #[inline]
    pub(crate) fn try_lookup(
        &self,
        wid: WidgetId,
        curr_hash: NodeHash,
        curr_avail: AvailableKey,
    ) -> Option<CachedSubtree<'_>> {
        let snap = self.snapshots.get(&wid)?;
        if snap.subtree_hash != curr_hash || snap.available_q != curr_avail {
            return None;
        }
        let nodes = snap.nodes.range();
        Some(CachedSubtree {
            root: self.nodes.desired[nodes.start],
            arenas: SubtreeArenas {
                desired: &self.nodes.desired[nodes.clone()],
                text_spans: &self.nodes.text_spans[nodes.clone()],
                text_spans_base: 0,
                scroll_content: &self.nodes.scroll_content[nodes],
                hugs: &self.hugs.items[snap.hugs.range()],
                text_shapes: &self.text_shapes_arena.items[snap.text_shapes.range()],
            },
        })
    }

    /// Overwrite (or insert) `wid`'s snapshot. `arenas.hugs` is the
    /// per-grid hug payload for every Grid descendant of the subtree,
    /// packed in `HUG_ORDER` (see grid module); empty for grid-free
    /// subtrees. Hot path is in-place memcpy when the existing range
    /// has the same length — expected to hit ~always once a widget
    /// reaches steady state, since `subtree_hash` includes structure
    /// (same hash → same subtree size). Size mismatches mark the old
    /// range as garbage and append a fresh range to the arena.
    pub(crate) fn write_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        available_q: AvailableKey,
        arenas: SubtreeArenas<'_>,
    ) {
        assert_eq!(arenas.desired.len(), arenas.text_spans.len());
        assert_eq!(arenas.desired.len(), arenas.scroll_content.len());
        assert!(!arenas.desired.is_empty(), "snapshot must include the root");
        let new_len = arenas.desired.len() as u32;
        let new_hugs_len = arenas.hugs.len() as u32;
        let new_text_len = arenas.text_shapes.len() as u32;

        if let Some(prev) = self.snapshots.get_mut(&wid)
            && prev.nodes.len == new_len
            && prev.hugs.len == new_hugs_len
            && prev.text_shapes.len == new_text_len
        {
            // In-place: hot path. Same `subtree_hash` → identical
            // structure → identical hug-array and text-shape-count
            // shape, so the existing ranges fit byte-for-byte.
            let nodes = prev.nodes.range();
            let hugs_range = prev.hugs.range();
            let text_range = prev.text_shapes.range();
            prev.subtree_hash = subtree_hash;
            prev.available_q = available_q;
            self.nodes.write_in_place(nodes, &arenas);
            self.hugs.items[hugs_range].copy_from_slice(arenas.hugs);
            self.text_shapes_arena.items[text_range].copy_from_slice(arenas.text_shapes);
            return;
        }

        // Different len (or first write): mark any existing range as
        // garbage, append the new one. Subtree size only changes when
        // a widget's structure changes, so this path is rare.
        if let Some(prev) = self.snapshots.get(&wid) {
            self.nodes.release(prev.nodes.len);
            self.hugs.release(prev.hugs.len);
            self.text_shapes_arena.release(prev.text_shapes.len);
        }
        let nodes_start = self.nodes.append(&arenas);
        let nodes = Span::new(nodes_start, new_len);
        let hugs_span = Span::new(self.hugs.items.len() as u32, new_hugs_len);
        self.hugs.items.extend_from_slice(arenas.hugs);
        let text_span = Span::new(self.text_shapes_arena.items.len() as u32, new_text_len);
        self.text_shapes_arena
            .items
            .extend_from_slice(arenas.text_shapes);
        self.nodes.acquire(new_len);
        self.hugs.acquire(new_hugs_len);
        self.text_shapes_arena.acquire(new_text_len);
        self.snapshots.insert(
            wid,
            ArenaSnapshot {
                subtree_hash,
                available_q,
                nodes,
                hugs: hugs_span,
                text_shapes: text_span,
            },
        );

        if self.nodes.needs_compact()
            || self.hugs.needs_compact()
            || self.text_shapes_arena.needs_compact()
        {
            self.compact();
        }
    }

    /// Drop snapshots for widgets that vanished this frame. The
    /// arena slots they referenced become garbage; a future
    /// `write_subtree` will compact them out once fragmentation
    /// crosses the threshold.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.nodes.release(snap.nodes.len);
                self.hugs.release(snap.hugs.len);
                self.text_shapes_arena.release(snap.text_shapes.len);
            }
        }
    }

    /// Walk every snapshot, copy its live range into a freshly-packed
    /// arena, and rewrite snapshot pointers. O(live) — runs at most
    /// once per ~N writes given `COMPACT_RATIO = 2`.
    fn compact(&mut self) {
        let mut new_nodes = NodeArenas::with_capacity(self.nodes.live);
        let mut new_hugs: Vec<f32> = Vec::with_capacity(self.hugs.live);
        let mut new_text_shapes: Vec<ShapedText> = Vec::with_capacity(self.text_shapes_arena.live);
        for snap in self.snapshots.values_mut() {
            snap.nodes.start = new_nodes.extend_from_within(&self.nodes, snap.nodes.range());
            let hugs = snap.hugs.range();
            snap.hugs.start = new_hugs.len() as u32;
            new_hugs.extend_from_slice(&self.hugs.items[hugs]);
            let text = snap.text_shapes.range();
            snap.text_shapes.start = new_text_shapes.len() as u32;
            new_text_shapes.extend_from_slice(&self.text_shapes_arena.items[text]);
        }
        new_nodes.live = self.nodes.live;
        self.nodes = new_nodes;
        self.hugs.items = new_hugs;
        self.text_shapes_arena.items = new_text_shapes;
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
