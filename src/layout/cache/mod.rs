//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `src/layout/measure-cache.md`.
//!
//! **Storage**: SoA arenas — two node-indexed and parallel, bundled
//! into [`NodeArenas`] (`desired`, `text_spans`) so length-equality is
//! structural; plus two variable-length per-subtree [`LiveArena`]s
//! (`hugs` for grid descendants, `text_shapes_arena` for
//! `ShapeRecord::Text` runs) — plus a tiny per-`WidgetId` `ArenaSnapshot`
//! pointing at a contiguous range. The dimensional cache key
//! (quantized `available`) lives directly on `ArenaSnapshot` as a
//! per-snapshot scalar, not in a parallel arena. Steady-state writes
//! are in-place memcpys when the subtree size matches; size mismatches
//! fall back to append + mark-garbage with periodic compaction.
//!
//! `NodeArenas` owns one shared `live` counter for its two columns;
//! each variable-length `LiveArena` tracks its own. Compaction
//! constants live in `src/common/live_arena.rs`.
//!
//! Compaction kicks in when an arena holds more than `live ×
//! COMPACT_RATIO` items. It slides every snapshot's live range down
//! over released garbage in place (`copy_within`) and truncates the
//! dead tail, rewriting each snapshot's `start` to its slid position —
//! no reallocation. O(live) — a one-frame cost paid infrequently.
//!
//! Eviction (via [`MeasureCache::sweep_removed`]) drops the snapshot
//! and releases its arena ranges; the slots stay as garbage until the
//! next compact.

use crate::common::content_hash::ContentHash;
use crate::common::live_arena::{COMPACT_FLOOR, COMPACT_RATIO, LiveArena};
use crate::layout::ShapedText;
use crate::layout::intrinsic::SLOT_COUNT;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::widget_id::WidgetIdMap;
use glam::IVec2;
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
    pub(crate) subtree_hash: ContentHash,
    /// Quantized `available` size at snapshot time — the dimensional
    /// half of the cache-validity check.
    pub(crate) available_q: AvailableKey,
    /// The snapshot root's per-slot intrinsic values (`[f32; SLOT_COUNT]`,
    /// X/Y × Min/Max-content). Served by [`MeasureCache::lookup_root_intrinsic`]
    /// to `LayoutEngine::intrinsic` so a re-measuring *parent*'s
    /// `children_max_intrinsic` reads the cached value instead of
    /// cold-recursing through this (unchanged) subtree — intrinsics are
    /// `available`-independent (computed at `available = ∞`), so a value
    /// keyed on `subtree_hash` stays valid across `available_q` buckets.
    /// A `NaN` slot is one the root never computed (e.g. a `Fixed` axis,
    /// or a `MaxContent` query that didn't fire); `lookup_root_intrinsic`
    /// returns `None` for it, so that slot recomputes on demand exactly as
    /// before.
    pub(crate) root_intrinsics: [f32; SLOT_COUNT],
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
    /// shaped text runs for every `ShapeRecord::Text` in the subtree, in
    /// pre-order. The `text_spans` slice (parallel to `desired`)
    /// stores **subtree-local** spans into this range.
    pub(crate) text_shapes: Span,
}

/// Quantized `available` size — the dimensional half of the cache
/// key. `i32::MAX` on either axis represents an infinite available
/// (ZStack / Hug parents propagate `f32::INFINITY`). Equality compare
/// is used as a cache-validity gate.
pub(crate) type AvailableKey = IVec2;

/// Per-subtree slice bundle: borrows into the two parallel
/// node-indexed arenas (`desired`, `text_spans`) plus the per-grid
/// `hugs` and the flat `text_shapes` payloads. The two node-indexed
/// slices share length and pre-order alignment; `hugs` is sized
/// per-grid descendant in `HUG_ORDER`; `text_shapes` is sized per
/// text-shape in pre-order.
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
    /// Per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// of the subtree, packed in pre-order. Each grid contributes
    /// four arrays in fixed order — cols.max, cols.min, rows.max,
    /// rows.min — for `2 * (n_cols + n_rows)` floats per grid.
    /// Empty for grid-free subtrees.
    pub(crate) hugs: &'a [f32],
    /// Flat per-text-shape buffer, in pre-order over the subtree's
    /// `ShapeRecord::Text` runs. Empty for text-free subtrees.
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
    debug_assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
    IVec2::new(quantize_axis(s.w), quantize_axis(s.h))
}

/// The two node-indexed parallel columns. Length-equality is
/// structural: every mutation goes through methods that touch both
/// at once, so the columns can't drift. `live` counts elements
/// still referenced by a snapshot; the underlying `Vec`s carry that
/// plus garbage from released snapshots until the next compact.
#[derive(Default)]
pub(crate) struct NodeArenas {
    pub(crate) desired: Vec<Size>,
    /// Per-node `Span` into each snapshot's flat `text_shapes_arena`
    /// range. Stored **subtree-local** (start relative to the snapshot's
    /// `text_shapes` range start) so spans survive flat-range compaction.
    pub(crate) text_spans: Vec<Span>,
    pub(crate) live: usize,
}

impl NodeArenas {
    fn write_in_place(&mut self, range: Range<usize>, src: &SubtreeArenas<'_>) {
        self.desired[range.clone()].copy_from_slice(src.desired);
        let base = src.text_spans_base;
        for (dst, s) in self.text_spans[range].iter_mut().zip(src.text_spans.iter()) {
            *dst = s.rebased(base);
        }
    }

    fn append(&mut self, src: &SubtreeArenas<'_>) -> u32 {
        let start = self.desired.len() as u32;
        self.desired.extend_from_slice(src.desired);
        let base = src.text_spans_base;
        self.text_spans
            .extend(src.text_spans.iter().map(|s| s.rebased(base)));
        start
    }

    fn acquire(&mut self, len: u32) {
        self.live += len as usize;
        debug_assert!(self.live <= self.desired.len());
    }

    pub(crate) fn release(&mut self, len: u32) {
        debug_assert!(self.live >= len as usize);
        self.live -= len as usize;
    }

    fn needs_compact(&self) -> bool {
        self.desired.len() > self.live.saturating_mul(COMPACT_RATIO) && self.live > COMPACT_FLOOR
    }
}

/// One live snapshot's arena spans, collected into
/// [`MeasureCache::compact_scratch`] and sorted by `nodes.start` to
/// drive the in-place repack.
#[derive(Clone, Copy, Debug)]
struct CompactEntry {
    wid: WidgetId,
    nodes: Span,
    hugs: Span,
    text: Span,
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
    /// Flat shaped-text buffer for every `ShapeRecord::Text` in every
    /// cached subtree, packed in pre-order. Snapshot records
    /// `(text_shapes_start, len)` into this arena. `text_spans`
    /// (per-node, subtree-local) addresses entries within each
    /// snapshot's range. Tracks its own live counter — text shapes
    /// don't appear on every node, so length doesn't ride on
    /// `desired.live`.
    pub(crate) text_shapes_arena: LiveArena<ShapedText>,
    /// Per-`WidgetId` snapshot index. Each value points at a range in
    /// the arenas above.
    pub(crate) snapshots: WidgetIdMap<ArenaSnapshot>,
    /// Reusable scratch for in-place [`Self::compact`]: one
    /// [`CompactEntry`] per live snapshot, sorted by `nodes.start`.
    /// Retained across frames (capacity reused) so compaction allocates
    /// nothing.
    compact_scratch: Vec<CompactEntry>,
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
        curr_hash: ContentHash,
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
                text_spans: &self.nodes.text_spans[nodes],
                text_spans_base: 0,
                hugs: &self.hugs.items[snap.hugs.range()],
                text_shapes: &self.text_shapes_arena.items[snap.text_shapes.range()],
            },
        })
    }

    /// Cross-frame intrinsic lookup for one node's subtree root. Unlike
    /// [`Self::try_lookup`] this ignores `available_q` — intrinsics are
    /// computed at `available = ∞`, so a value is valid for any available
    /// as long as `subtree_hash` matches. Returns `None` when there's no
    /// snapshot for `wid`, the hash differs, or the slot was never
    /// populated (`NaN`). Lets `LayoutEngine::intrinsic` answer a query
    /// from last frame's snapshot instead of recursing through an
    /// unchanged subtree — even on a resize frame where `try_lookup`
    /// misses on the dimensional half of the key.
    #[inline]
    pub(crate) fn lookup_root_intrinsic(
        &self,
        wid: WidgetId,
        subtree_hash: ContentHash,
        slot: usize,
    ) -> Option<f32> {
        let snap = self.snapshots.get(&wid)?;
        if snap.subtree_hash != subtree_hash {
            return None;
        }
        let v = snap.root_intrinsics[slot];
        (!v.is_nan()).then_some(v)
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
        subtree_hash: ContentHash,
        available_q: AvailableKey,
        root_intrinsics: [f32; SLOT_COUNT],
        arenas: SubtreeArenas<'_>,
    ) {
        debug_assert_eq!(arenas.desired.len(), arenas.text_spans.len());
        debug_assert!(!arenas.desired.is_empty(), "snapshot must include the root");
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
            prev.root_intrinsics = root_intrinsics;
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
                root_intrinsics,
                nodes,
                hugs: hugs_span,
                text_shapes: text_span,
            },
        );
    }

    /// Run compaction if any arena's `len > live × COMPACT_RATIO`
    /// (and `live > COMPACT_FLOOR`). Called from
    /// `LayoutEngine::sweep_removed` only — acquires grow `desired`
    /// and `live` in lockstep so writes can't be the trigger, only
    /// releases can.
    pub(crate) fn maybe_compact(&mut self) {
        if self.nodes.needs_compact()
            || self.hugs.needs_compact()
            || self.text_shapes_arena.needs_compact()
        {
            self.compact();
        }
    }

    /// Repack the three arenas **in place**, sliding each live
    /// snapshot's ranges down over released garbage and truncating the
    /// dead tail. O(live) `copy_within`, no allocation — `truncate`
    /// keeps the `Vec` capacities. Runs at most once per ~N writes given
    /// `COMPACT_RATIO = 2`.
    ///
    /// Snapshots are processed in ascending `nodes.start` order, so for
    /// every arena the cumulative live length of earlier snapshots is
    /// `<=` the current snapshot's source start — each `copy_within`
    /// writes toward the front (`dst <= src`), never clobbering an
    /// unmoved snapshot. The three arenas share one order: `write_subtree`
    /// appends to (and rewrites in place) all three in lockstep, and this
    /// compaction preserves that order, so sorting by `nodes.start` also
    /// orders `hugs` and `text_shapes`.
    ///
    /// (The earlier implementation rebuilt all arenas with
    /// `Vec::with_capacity`; on a resize workload this fires ~every frame
    /// — `LayoutEngine::sweep_removed` was the #2 allocator in
    /// `alloc_resize`'s dhat dump — so the fresh-`Vec` rebuild was pure
    /// per-frame churn.)
    fn compact(&mut self) {
        let mut scratch = std::mem::take(&mut self.compact_scratch);
        scratch.clear();
        scratch.extend(self.snapshots.iter().map(|(wid, snap)| CompactEntry {
            wid: *wid,
            nodes: snap.nodes,
            hugs: snap.hugs,
            text: snap.text_shapes,
        }));
        scratch.sort_unstable_by_key(|e| e.nodes.start);

        let mut node_w = 0u32;
        let mut hug_w = 0u32;
        let mut text_w = 0u32;
        for e in scratch.iter() {
            if e.nodes.start != node_w {
                let src = e.nodes.range();
                let dst = node_w as usize;
                self.nodes.desired.copy_within(src.clone(), dst);
                self.nodes.text_spans.copy_within(src, dst);
            }
            if e.hugs.start != hug_w {
                self.hugs.items.copy_within(e.hugs.range(), hug_w as usize);
            }
            if e.text.start != text_w {
                self.text_shapes_arena
                    .items
                    .copy_within(e.text.range(), text_w as usize);
            }
            let snap = self
                .snapshots
                .get_mut(&e.wid)
                .expect("snapshot present: scratch built from snapshots this call");
            snap.nodes = Span::new(node_w, e.nodes.len);
            snap.hugs = Span::new(hug_w, e.hugs.len);
            snap.text_shapes = Span::new(text_w, e.text.len);
            node_w += e.nodes.len;
            hug_w += e.hugs.len;
            text_w += e.text.len;
        }

        // `live` is unchanged — only the garbage tail is dropped.
        self.nodes.desired.truncate(node_w as usize);
        self.nodes.text_spans.truncate(node_w as usize);
        self.hugs.items.truncate(hug_w as usize);
        self.text_shapes_arena.items.truncate(text_w as usize);

        self.compact_scratch = scratch;
    }

    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.nodes.clear();
        self.hugs.clear();
        self.text_shapes_arena.clear();
        self.snapshots.clear();
    }
}

/// Test/bench reach-in, gated so the shipping build sees no dead code
/// (reachable only from the `internals`-gated cache `clear()` paths).
#[cfg(any(test, feature = "internals"))]
impl NodeArenas {
    pub(crate) fn clear(&mut self) {
        self.desired.clear();
        self.text_spans.clear();
        self.live = 0;
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
