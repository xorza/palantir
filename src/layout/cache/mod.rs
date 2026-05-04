//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `src/layout/measure-cache.md`.
//!
//! **Storage**: a single SoA arena per attribute (`desired_arena`,
//! `text_arena`) plus a tiny per-`WidgetId` `ArenaSnapshot` pointing
//! at a contiguous range. Steady-state writes are in-place memcpys
//! when the subtree size matches; size mismatches fall back to
//! append + mark-garbage with periodic compaction. This keeps total
//! allocations bounded to two `Vec`s (the arenas) plus one
//! `FxHashMap` (the snapshot index), regardless of widget count.
//!
//! Compaction kicks in when the arena holds more than `2 ×
//! live_entries`. It walks every snapshot, rewrites their `start`
//! indices to point at a freshly-packed arena, and drops the old
//! one. O(live_entries) — a one-frame cost paid infrequently.
//!
//! Eviction (via [`MeasureCache::sweep_removed`]) drops the snapshot
//! and decrements `live_entries`; the arena slot stays as garbage
//! until the next compact.

use crate::layout::result::ShapedText;
use crate::layout::types::span::Span;
use crate::primitives::size::Size;
use crate::tree::hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::IVec2;
use rustc_hash::FxHashMap;

/// 24-byte snapshot. `nodes` indexes the three node-indexed arenas
/// (`desired_arena`, `text_arena`, `available_arena`); `hugs` indexes
/// `hugs_arena`. The snapshot's quantized `available` is recoverable
/// as `available_arena[nodes.start]` (always the snapshot root's
/// per-node entry) — no separate field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub(crate) subtree_hash: NodeHash,
    /// Range over the three node-indexed arenas. `desired_arena[nodes.range()]`
    /// is the subtree's `desired` in pre-order; index 0 is the
    /// snapshot root's own size.
    pub(crate) nodes: Span,
    /// Range over `hugs_arena`. Per-grid hug arrays for every
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

/// Compaction trigger: arena length must exceed `live_entries × this`.
/// `2.0` keeps fragmentation under one extra arena's worth without
/// firing compactions on every miss frame.
const COMPACT_RATIO: usize = 2;
/// Don't bother compacting until live data is at least this many
/// entries — avoids compaction spam at warmup when the arena is tiny.
const COMPACT_FLOOR: usize = 64;

#[derive(Default)]
pub(crate) struct MeasureCache {
    /// Backing storage for every snapshot's `desired` data. Live
    /// regions are pointed at by `snapshots`; freed regions sit as
    /// garbage until the next [`Self::compact`].
    pub(crate) desired_arena: Vec<Size>,
    /// Parallel to `desired_arena`. Same indexing.
    pub(crate) text_arena: Vec<Option<ShapedText>>,
    /// Parallel to `desired_arena`. Same indexing. Per-descendant
    /// quantized `available`, snapshotted so a measure-cache hit can
    /// restore the full subtree's `available_q` column on
    /// `LayoutScratch`. The encode cache reads it at every node it
    /// visits, so descendants must remain correct even when the
    /// measure pass short-circuits and never visits them.
    pub(crate) available_arena: Vec<AvailableKey>,
    /// Per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// of every cached subtree, packed in pre-order. Snapshot
    /// records `(hugs_start, hugs_len)` into this arena. Lets a
    /// cache hit restore `LayoutEngine.scratch.grid.hugs` for the
    /// cached subtree's grids without walking children —
    /// `grid::arrange` then resolves track sizes correctly. Without
    /// this, a cache hit at any ancestor of a Grid would leave `hugs`
    /// zeroed and the grid would collapse every cell to (0, 0).
    pub(crate) hugs_arena: Vec<f32>,
    /// Per-`WidgetId` snapshot index. Each value points at a range in
    /// the two arenas above.
    pub(crate) snapshots: FxHashMap<WidgetId, ArenaSnapshot>,
    /// Sum of `snap.len` across `snapshots` — the total live data in
    /// the arenas. Garbage = `desired_arena.len() - live_entries`.
    pub(crate) live_entries: usize,
    /// Reusable buffer for the next snapshot's hug payload. Caller
    /// fills via [`GridHugStore::snapshot_subtree`] before invoking
    /// [`Self::write_subtree`]; `write_subtree` consumes it. Capacity
    /// retained across frames so steady-state writes don't allocate.
    pub(crate) tmp_hugs: Vec<f32>,
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
        // Snapshot's `available_q` lives at `available_arena[nodes.start]` —
        // the per-node entry for the snapshot root, written at the
        // same time as the field used to live.
        if snap.subtree_hash != curr_hash || self.available_arena[nodes.start] != curr_avail {
            return None;
        }
        Some(CachedSubtree {
            root: self.desired_arena[nodes.start],
            desired: &self.desired_arena[nodes.clone()],
            text_shapes: &self.text_arena[nodes.clone()],
            available_q: &self.available_arena[nodes],
            hugs: &self.hugs_arena[snap.hugs.range()],
        })
    }

    /// Overwrite (or insert) `wid`'s snapshot. Caller must have first
    /// populated [`Self::tmp_hugs`] with the per-grid hug arrays for
    /// every Grid descendant, in `HUG_ORDER` (see grid module). Hot
    /// path is in-place memcpy when the existing range has the same
    /// length — expected to hit ~always once a widget reaches steady
    /// state, since `subtree_hash` includes structure (same hash →
    /// same subtree size). Size mismatches mark the old range as
    /// garbage and append a fresh range to the arena.
    pub(crate) fn write_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        desired: &[Size],
        text_shapes: &[Option<ShapedText>],
        available_qs: &[AvailableKey],
    ) {
        assert_eq!(desired.len(), text_shapes.len());
        assert_eq!(desired.len(), available_qs.len());
        assert!(
            !available_qs.is_empty(),
            "snapshot must include the root's own per-node available_q",
        );
        let new_len = desired.len() as u32;
        let new_hugs_len = self.tmp_hugs.len() as u32;

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
            // Disjoint-field borrows: arenas vs. tmp_hugs.
            let Self {
                desired_arena,
                text_arena,
                available_arena,
                hugs_arena,
                tmp_hugs,
                ..
            } = self;
            desired_arena[nodes.clone()].copy_from_slice(desired);
            text_arena[nodes.clone()].copy_from_slice(text_shapes);
            available_arena[nodes].copy_from_slice(available_qs);
            hugs_arena[hugs_range].copy_from_slice(tmp_hugs);
            return;
        }

        // Different len (or first write): mark any existing range as
        // garbage, append the new one. Subtree size only changes when
        // a widget's structure changes, so this path is rare.
        if let Some(prev) = self.snapshots.get(&wid) {
            self.live_entries -= prev.nodes.len as usize;
        }
        let nodes = Span::new(self.desired_arena.len() as u32, new_len);
        self.desired_arena.extend_from_slice(desired);
        self.text_arena.extend_from_slice(text_shapes);
        self.available_arena.extend_from_slice(available_qs);
        let hugs_span = Span::new(self.hugs_arena.len() as u32, new_hugs_len);
        self.hugs_arena.extend_from_slice(&self.tmp_hugs);
        self.live_entries += new_len as usize;
        self.snapshots.insert(
            wid,
            ArenaSnapshot {
                subtree_hash,
                nodes,
                hugs: hugs_span,
            },
        );

        if self.desired_arena.len() > self.live_entries.saturating_mul(COMPACT_RATIO)
            && self.live_entries > COMPACT_FLOOR
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
                self.live_entries -= snap.nodes.len as usize;
            }
        }
    }

    /// Drop every cross-frame snapshot. Public via
    /// `Ui::__clear_measure_cache` for benchmarks.
    pub(crate) fn clear(&mut self) {
        self.desired_arena.clear();
        self.text_arena.clear();
        self.available_arena.clear();
        self.hugs_arena.clear();
        self.tmp_hugs.clear();
        self.snapshots.clear();
        self.live_entries = 0;
    }

    /// Walk every snapshot, copy its live range into a freshly-packed
    /// arena, and rewrite snapshot pointers. O(live_entries) — runs
    /// at most once per ~N writes given `COMPACT_RATIO = 2`.
    fn compact(&mut self) {
        let Self {
            desired_arena,
            text_arena,
            available_arena,
            hugs_arena,
            snapshots,
            live_entries,
            tmp_hugs: _,
        } = self;
        let mut new_desired: Vec<Size> = Vec::with_capacity(*live_entries);
        let mut new_text: Vec<Option<ShapedText>> = Vec::with_capacity(*live_entries);
        let mut new_avail: Vec<AvailableKey> = Vec::with_capacity(*live_entries);
        let mut new_hugs: Vec<f32> = Vec::with_capacity(hugs_arena.len());
        for snap in snapshots.values_mut() {
            let nodes = snap.nodes.range();
            snap.nodes.start = new_desired.len() as u32;
            new_desired.extend_from_slice(&desired_arena[nodes.clone()]);
            new_text.extend_from_slice(&text_arena[nodes.clone()]);
            new_avail.extend_from_slice(&available_arena[nodes]);
            let hugs = snap.hugs.range();
            snap.hugs.start = new_hugs.len() as u32;
            new_hugs.extend_from_slice(&hugs_arena[hugs]);
        }
        *desired_arena = new_desired;
        *text_arena = new_text;
        *available_arena = new_avail;
        *hugs_arena = new_hugs;
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
