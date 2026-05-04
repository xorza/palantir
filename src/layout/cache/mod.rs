//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `src/layout/measure-cache.md`.
//!
//! **Storage**: SoA arenas (`desired`, `text`, `available` — all
//! node-indexed and parallel — and `hugs` for grid descendants) plus
//! a tiny per-`WidgetId` `ArenaSnapshot` pointing at a contiguous
//! range. Steady-state writes are in-place memcpys when the subtree
//! size matches; size mismatches fall back to append + mark-garbage
//! with periodic compaction. Storage is a small set of `Vec`s plus
//! one `FxHashMap` (the snapshot index), regardless of widget count.
//!
//! Liveness bookkeeping rides on the shared [`LiveArena`] primitive
//! so the compaction policy and constants are in one place
//! (`src/common/cache_arena.rs`); the three node-indexed arenas share
//! `desired.live` (they grow and shrink together by invariant), `hugs`
//! tracks its own.
//!
//! Compaction kicks in when an arena holds more than `live ×
//! COMPACT_RATIO` items. It walks every snapshot, rewrites their
//! `start` indices to point at a freshly-packed arena, and drops the
//! old one. O(live) — a one-frame cost paid infrequently.
//!
//! Eviction (via [`MeasureCache::sweep_removed`]) drops the snapshot
//! and releases its arena ranges; the slots stay as garbage until the
//! next compact.

use crate::common::cache_arena::LiveArena;
use crate::layout::result::ShapedText;
use crate::layout::types::span::Span;
use crate::primitives::size::Size;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::IVec2;
use rustc_hash::FxHashMap;

/// 24-byte snapshot. `nodes` indexes the three node-indexed arenas
/// (`desired`, `text`, `available`); `hugs` indexes `hugs`. The
/// snapshot's quantized `available` is recoverable as
/// `available[nodes.start]` (always the snapshot root's per-node
/// entry) — no separate field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub(crate) subtree_hash: NodeHash,
    /// Range over the three node-indexed arenas. `desired.items[nodes.range()]`
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

/// What [`MeasureCache::try_lookup`] returns on a hit. The slices are
/// borrows into the cache's arenas, ready to `copy_from_slice` into
/// the caller's destination columns. `root` is the snapshot root's
/// own `desired` — the value `measure` returns up the recursion.
pub(crate) struct CachedSubtree<'a> {
    pub(crate) root: Size,
    pub(crate) desired: &'a [Size],
    pub(crate) text_shapes: &'a [Option<ShapedText>],
    pub(crate) available_q: &'a [AvailableKey],
    /// Sequential slice of f32s; consumed in pre-order by walking
    /// the subtree and pulling `2 * (n_cols + n_rows)` per
    /// `LayoutMode::Grid` descendant. Empty for grid-free subtrees.
    pub(crate) hugs: &'a [f32],
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
    IVec2::new(quantize_axis(s.w), quantize_axis(s.h))
}

#[derive(Default)]
pub(crate) struct MeasureCache {
    /// Owns the live counter shared across the three parallel
    /// node-indexed arenas. `text` and `available` ride on
    /// `desired.live` by the same-length invariant.
    pub(crate) desired: LiveArena<Size>,
    /// Parallel to `desired`. Same indexing.
    pub(crate) text: Vec<Option<ShapedText>>,
    /// Parallel to `desired`. Same indexing. Per-descendant
    /// quantized `available`, snapshotted so a measure-cache hit can
    /// restore the full subtree's `available_q` column on
    /// `LayoutScratch`. The encode cache reads it at every node it
    /// visits, so descendants must remain correct even when the
    /// measure pass short-circuits and never visits them.
    pub(crate) available: Vec<AvailableKey>,
    /// Per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// of every cached subtree, packed in pre-order. Snapshot
    /// records `(hugs_start, hugs_len)` into this arena. Lets a
    /// cache hit restore `LayoutEngine.scratch.grid.hugs` for the
    /// cached subtree's grids without walking children —
    /// `grid::arrange` then resolves track sizes correctly. Without
    /// this, a cache hit at any ancestor of a Grid would leave `hugs`
    /// zeroed and the grid would collapse every cell to (0, 0).
    pub(crate) hugs: LiveArena<f32>,
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
        // Snapshot's `available_q` lives at `available[nodes.start]` —
        // the per-node entry for the snapshot root, written at the
        // same time as the field used to live.
        if snap.subtree_hash != curr_hash || self.available[nodes.start] != curr_avail {
            return None;
        }
        Some(CachedSubtree {
            root: self.desired.items[nodes.start],
            desired: &self.desired.items[nodes.clone()],
            text_shapes: &self.text[nodes.clone()],
            available_q: &self.available[nodes],
            hugs: &self.hugs.items[snap.hugs.range()],
        })
    }

    /// Overwrite (or insert) `wid`'s snapshot. `hugs` is the per-grid
    /// hug payload for every Grid descendant of the subtree, packed
    /// in `HUG_ORDER` (see grid module); empty for grid-free
    /// subtrees. Hot path is in-place memcpy when the existing range
    /// has the same length — expected to hit ~always once a widget
    /// reaches steady state, since `subtree_hash` includes structure
    /// (same hash → same subtree size). Size mismatches mark the old
    /// range as garbage and append a fresh range to the arena.
    pub(crate) fn write_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        desired: &[Size],
        text_shapes: &[Option<ShapedText>],
        available_qs: &[AvailableKey],
        hugs: &[f32],
    ) {
        assert_eq!(desired.len(), text_shapes.len());
        assert_eq!(desired.len(), available_qs.len());
        assert!(
            !available_qs.is_empty(),
            "snapshot must include the root's own per-node available_q",
        );
        let new_len = desired.len() as u32;
        let new_hugs_len = hugs.len() as u32;

        if let Some(prev) = self.snapshots.get_mut(&wid)
            && prev.nodes.len == new_len
            && prev.hugs.len == new_hugs_len
        {
            // In-place: hot path. Same `subtree_hash` → identical
            // structure → identical hug-array shape, so the existing
            // ranges fit byte-for-byte.
            let nodes = prev.nodes.range();
            let hugs_range = prev.hugs.range();
            prev.subtree_hash = subtree_hash;
            self.desired.items[nodes.clone()].copy_from_slice(desired);
            self.text[nodes.clone()].copy_from_slice(text_shapes);
            self.available[nodes].copy_from_slice(available_qs);
            self.hugs.items[hugs_range].copy_from_slice(hugs);
            return;
        }

        // Different len (or first write): mark any existing range as
        // garbage, append the new one. Subtree size only changes when
        // a widget's structure changes, so this path is rare.
        if let Some(prev) = self.snapshots.get(&wid) {
            self.desired.release(prev.nodes.len);
            self.hugs.release(prev.hugs.len);
        }
        let nodes = Span::new(self.desired.items.len() as u32, new_len);
        self.desired.items.extend_from_slice(desired);
        self.text.extend_from_slice(text_shapes);
        self.available.extend_from_slice(available_qs);
        let hugs_span = Span::new(self.hugs.items.len() as u32, new_hugs_len);
        self.hugs.items.extend_from_slice(hugs);
        self.desired.live += new_len as usize;
        self.hugs.live += new_hugs_len as usize;
        self.snapshots.insert(
            wid,
            ArenaSnapshot {
                subtree_hash,
                nodes,
                hugs: hugs_span,
            },
        );

        if self.desired.needs_compact() || self.hugs.needs_compact() {
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
            }
        }
    }

    /// Drop every cross-frame snapshot. Reachable only via
    /// `internals::clear_measure_cache` (gated to tests + the
    /// `internals` feature) — not part of any production code path.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.desired.clear();
        self.text.clear();
        self.available.clear();
        self.hugs.clear();
        self.snapshots.clear();
    }

    /// Walk every snapshot, copy its live range into a freshly-packed
    /// arena, and rewrite snapshot pointers. O(live) — runs at most
    /// once per ~N writes given `COMPACT_RATIO = 2`.
    fn compact(&mut self) {
        let mut new_desired: Vec<Size> = Vec::with_capacity(self.desired.live);
        let mut new_text: Vec<Option<ShapedText>> = Vec::with_capacity(self.desired.live);
        let mut new_avail: Vec<AvailableKey> = Vec::with_capacity(self.desired.live);
        let mut new_hugs: Vec<f32> = Vec::with_capacity(self.hugs.live);
        for snap in self.snapshots.values_mut() {
            let nodes = snap.nodes.range();
            snap.nodes.start = new_desired.len() as u32;
            new_desired.extend_from_slice(&self.desired.items[nodes.clone()]);
            new_text.extend_from_slice(&self.text[nodes.clone()]);
            new_avail.extend_from_slice(&self.available[nodes]);
            let hugs = snap.hugs.range();
            snap.hugs.start = new_hugs.len() as u32;
            new_hugs.extend_from_slice(&self.hugs.items[hugs]);
        }
        self.desired.items = new_desired;
        self.text = new_text;
        self.available = new_avail;
        self.hugs.items = new_hugs;
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
