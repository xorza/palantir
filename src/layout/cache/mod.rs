//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `src/layout/measure-cache.md`.
//!
//! **Storage**: SoA arenas — four node-indexed and parallel
//! (`desired`, `text_spans`, `available`, `scroll_content`) plus two
//! variable-length per-subtree (`hugs` for grid descendants,
//! `text_shapes_arena` for `Shape::Text` runs) — plus a tiny
//! per-`WidgetId` `ArenaSnapshot` pointing at a contiguous range.
//! Steady-state writes are in-place memcpys when the subtree size
//! matches; size mismatches fall back to append + mark-garbage with
//! periodic compaction. Storage is a small set of `Vec`s plus one
//! `FxHashMap` (the snapshot index), regardless of widget count.
//!
//! Liveness bookkeeping rides on the shared [`LiveArena`] primitive
//! so the compaction policy and constants are in one place
//! (`src/common/cache_arena.rs`); the four node-indexed arenas share
//! `desired.live` (they grow and shrink together by invariant); each
//! variable-length arena (`hugs`, `text_shapes_arena`) tracks its own.
//!
//! Compaction kicks in when an arena holds more than `live ×
//! COMPACT_RATIO` items. It walks every snapshot, rewrites their
//! `start` indices to point at a freshly-packed arena, and drops the
//! old one. O(live) — a one-frame cost paid infrequently.
//!
//! Eviction (via [`MeasureCache::sweep_removed`]) drops the snapshot
//! and releases its arena ranges; the slots stay as garbage until the
//! next compact.
//!
//! ## `available_q` lives in two places
//!
//! 1. [`MeasureCache::available`] — per-node arena, parallel to
//!    `desired`. Stores every cached subtree's per-node quantized
//!    `available`. The root entry is read at lookup time as the
//!    dimensional half of the cache-validity check; the descendant
//!    entries are restored on a cache hit so consumers downstream see
//!    a populated column even for subtrees the measure pass
//!    short-circuited.
//! 2. [`crate::layout::result::LayoutResult::available_q`] — per-node,
//!    per-frame. Written by `LayoutEngine::measure` on every node it
//!    visits, restored from the arena copy on a cache hit so descendants
//!    skipped by the short-circuit still carry their value. Read by
//!    downstream consumers (encode cache, etc.) at every visited node.
//!
//! The data flow on a hit: parent's `measure` looks up the snapshot,
//! restores `desired` + `text_shapes` + `available_q` + `hugs` slices
//! into per-frame storage, returns the root size — descendants are
//! never visited but their per-frame state matches as if they had been.

use crate::common::cache_arena::LiveArena;
use crate::layout::result::ShapedText;
use crate::layout::types::span::Span;
use crate::primitives::size::Size;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::IVec2;
use rustc_hash::FxHashMap;

/// Snapshot index entry. `nodes` indexes the node-indexed arenas
/// (`desired`, `text_spans`, `available`, `scroll_content`); `hugs`
/// indexes `hugs`; `text_shapes` indexes `text_shapes_arena`. The
/// snapshot key is `(subtree_hash, available[nodes.start])` — the
/// dimensional half is read out of the per-node `available` arena,
/// which would be carrying the same value on a redundant field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub(crate) subtree_hash: NodeHash,
    /// Range over the node-indexed arenas. `desired.items[nodes.range()]`
    /// is the subtree's `desired` in pre-order; index 0 is the
    /// snapshot root's own size.
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

/// Sentinel "never written" value. Distinct from anything
/// [`quantize_available`] can produce: that function emits `i32::MAX`
/// for infinity or `>= 0` for finite (the inputs are always
/// non-negative `available` sizes), so `i32::MIN` cannot collide with
/// a real key. Used as the per-frame init fill for
/// `LayoutScratch.available_q` so a cache-validity equality check can
/// never spuriously match against a slot whose write was somehow
/// skipped — the `{0, 0}` zero default would compare equal to a
/// legitimately-stored 0px × 0px snapshot.
pub(crate) const AVAIL_UNSET: AvailableKey = IVec2::splat(i32::MIN);

/// Per-subtree slice bundle: borrows into the four parallel
/// node-indexed arenas (`desired`, `text_spans`, `available_q`,
/// `scroll_content`) plus the per-grid `hugs` and the flat
/// `text_shapes` payloads. The four node-indexed slices share length
/// and pre-order alignment; `hugs` is sized per-grid descendant in
/// `HUG_ORDER`; `text_shapes` is sized per text-shape in pre-order.
/// `text_spans` stores **subtree-local** spans into `text_shapes`
/// (start relative to the slice's first element). Same shape for
/// both reads ([`MeasureCache::try_lookup`]) and writes
/// ([`MeasureCache::write_subtree`]).
pub(crate) struct SubtreeArenas<'a> {
    pub(crate) desired: &'a [Size],
    /// Per-node `Span` into `text_shapes`. Subtree-local: span
    /// `start` is relative to `text_shapes[0]`.
    pub(crate) text_spans: &'a [Span],
    pub(crate) available_q: &'a [AvailableKey],
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
    // Non-negative inputs are load-bearing for the `AVAIL_UNSET = i32::MIN`
    // sentinel: a negative `available` could quantize to `i32::MIN` and
    // collide with the sentinel. Layout invariants keep `available` in
    // `[0, ∞)`; pin the contract here so a future regression trips early.
    assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
    IVec2::new(quantize_axis(s.w), quantize_axis(s.h))
}

#[derive(Default)]
pub(crate) struct MeasureCache {
    /// Owns the live counter shared across the parallel node-indexed
    /// arenas. `text_spans`, `available`, and `scroll_content` ride
    /// on `desired.live` by the same-length invariant.
    pub(crate) desired: LiveArena<Size>,
    /// Parallel to `desired`. Same indexing. Per-node `Span` into
    /// each snapshot's flat `text_shapes_arena` range. Spans are
    /// stored **subtree-local** (start relative to the snapshot's
    /// `text_shapes` range start) so they remain valid after
    /// compaction relocates the flat range.
    pub(crate) text_spans: Vec<Span>,
    /// Parallel to `desired`. Same indexing. Per-descendant
    /// quantized `available`, snapshotted so a measure-cache hit can
    /// restore the full subtree's `available_q` column on
    /// `LayoutScratch`. The encode cache reads it at every node it
    /// visits, so descendants must remain correct even when the
    /// measure pass short-circuits and never visits them.
    pub(crate) available: Vec<AvailableKey>,
    /// Parallel to `desired`. Same indexing. Per-node measured
    /// content extent for `LayoutMode::Scroll` nodes (zero for
    /// non-scroll). Snapshotted so a cache hit can restore the
    /// `LayoutResult.scroll_content` slice for the subtree without
    /// re-running the underlying stack/zstack measure.
    pub(crate) scroll_content: Vec<Size>,
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
    /// `(subtree_hash, available_q)`. On hit, return a
    /// [`CachedSubtree`] with the root's `desired` and the two
    /// arena slices ready to copy. On miss, `None`.
    #[inline]
    pub(crate) fn try_lookup(
        &self,
        wid: WidgetId,
        curr_hash: NodeHash,
        curr_avail: AvailableKey,
    ) -> Option<CachedSubtree<'_>> {
        let snap = self.snapshots.get(&wid)?;
        let nodes = snap.nodes.range();
        if snap.subtree_hash != curr_hash || self.available[nodes.start] != curr_avail {
            return None;
        }
        Some(CachedSubtree {
            root: self.desired.items[nodes.start],
            arenas: SubtreeArenas {
                desired: &self.desired.items[nodes.clone()],
                text_spans: &self.text_spans[nodes.clone()],
                available_q: &self.available[nodes.clone()],
                scroll_content: &self.scroll_content[nodes],
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
        arenas: SubtreeArenas<'_>,
    ) {
        let SubtreeArenas {
            desired,
            text_spans,
            available_q,
            scroll_content,
            hugs,
            text_shapes,
        } = arenas;
        assert_eq!(desired.len(), text_spans.len());
        assert_eq!(desired.len(), available_q.len());
        assert_eq!(desired.len(), scroll_content.len());
        assert!(
            !available_q.is_empty(),
            "snapshot must include the root's own per-node available_q",
        );
        let new_len = desired.len() as u32;
        let new_hugs_len = hugs.len() as u32;
        let new_text_len = text_shapes.len() as u32;

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
            self.desired.items[nodes.clone()].copy_from_slice(desired);
            self.text_spans[nodes.clone()].copy_from_slice(text_spans);
            self.available[nodes.clone()].copy_from_slice(available_q);
            self.scroll_content[nodes].copy_from_slice(scroll_content);
            self.hugs.items[hugs_range].copy_from_slice(hugs);
            self.text_shapes_arena.items[text_range].copy_from_slice(text_shapes);
            return;
        }

        // Different len (or first write): mark any existing range as
        // garbage, append the new one. Subtree size only changes when
        // a widget's structure changes, so this path is rare.
        if let Some(prev) = self.snapshots.get(&wid) {
            self.desired.release(prev.nodes.len);
            self.hugs.release(prev.hugs.len);
            self.text_shapes_arena.release(prev.text_shapes.len);
        }
        let nodes = Span::new(self.desired.items.len() as u32, new_len);
        self.desired.items.extend_from_slice(desired);
        self.text_spans.extend_from_slice(text_spans);
        self.available.extend_from_slice(available_q);
        self.scroll_content.extend_from_slice(scroll_content);
        let hugs_span = Span::new(self.hugs.items.len() as u32, new_hugs_len);
        self.hugs.items.extend_from_slice(hugs);
        let text_span = Span::new(self.text_shapes_arena.items.len() as u32, new_text_len);
        self.text_shapes_arena.items.extend_from_slice(text_shapes);
        self.desired.acquire(new_len);
        self.hugs.acquire(new_hugs_len);
        self.text_shapes_arena.acquire(new_text_len);
        self.snapshots.insert(
            wid,
            ArenaSnapshot {
                subtree_hash,
                nodes,
                hugs: hugs_span,
                text_shapes: text_span,
            },
        );

        if self.desired.needs_compact()
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
                self.desired.release(snap.nodes.len);
                self.hugs.release(snap.hugs.len);
                self.text_shapes_arena.release(snap.text_shapes.len);
            }
        }
    }

    /// Walk every snapshot, copy its live range into a freshly-packed
    /// arena, and rewrite snapshot pointers. O(live) — runs at most
    /// once per ~N writes given `COMPACT_RATIO = 2`.
    fn compact(&mut self) {
        let mut new_desired: Vec<Size> = Vec::with_capacity(self.desired.live);
        let mut new_text_spans: Vec<Span> = Vec::with_capacity(self.desired.live);
        let mut new_avail: Vec<AvailableKey> = Vec::with_capacity(self.desired.live);
        let mut new_scroll: Vec<Size> = Vec::with_capacity(self.desired.live);
        let mut new_hugs: Vec<f32> = Vec::with_capacity(self.hugs.live);
        let mut new_text_shapes: Vec<ShapedText> = Vec::with_capacity(self.text_shapes_arena.live);
        for snap in self.snapshots.values_mut() {
            let nodes = snap.nodes.range();
            snap.nodes.start = new_desired.len() as u32;
            new_desired.extend_from_slice(&self.desired.items[nodes.clone()]);
            new_text_spans.extend_from_slice(&self.text_spans[nodes.clone()]);
            new_avail.extend_from_slice(&self.available[nodes.clone()]);
            new_scroll.extend_from_slice(&self.scroll_content[nodes]);
            let hugs = snap.hugs.range();
            snap.hugs.start = new_hugs.len() as u32;
            new_hugs.extend_from_slice(&self.hugs.items[hugs]);
            let text = snap.text_shapes.range();
            snap.text_shapes.start = new_text_shapes.len() as u32;
            new_text_shapes.extend_from_slice(&self.text_shapes_arena.items[text]);
        }
        self.desired.items = new_desired;
        self.text_spans = new_text_spans;
        self.available = new_avail;
        self.scroll_content = new_scroll;
        self.hugs.items = new_hugs;
        self.text_shapes_arena.items = new_text_shapes;
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
