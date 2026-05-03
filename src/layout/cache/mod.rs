//! Cross-frame measure cache (Phase 2: full subtree skip). Skip a
//! node's *entire subtree* — body and recursion — when its
//! `subtree_hash` and incoming `available` size both match last
//! frame. See `docs/measure-cache.md`.
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
use crate::primitives::{Size, WidgetId};
use crate::tree::NodeHash;
use rustc_hash::FxHashMap;
use std::ops::Range;

/// 24-byte snapshot. `start..start+len` indexes both arenas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    /// Rolled subtree hash from last frame. The rollup includes child
    /// count and per-child subtree hashes, so any structural or
    /// authoring change anywhere in the subtree busts the key.
    pub subtree_hash: NodeHash,
    /// `available` size passed to this node's measure last frame,
    /// quantized to integer logical pixels.
    pub available_q: AvailableKey,
    /// Range start into both `desired_arena` and `text_arena`.
    pub start: u32,
    /// Range length (number of nodes in the snapshot's subtree).
    /// `desired_arena[start..start+len]` is the subtree's `desired`
    /// in pre-order; index 0 (i.e. `desired_arena[start]`) is the
    /// snapshot root's own size.
    pub len: u32,
}

/// Quantized (`available.w`, `available.h`) — the dimensional half of
/// the cache key. `i32::MAX` represents an infinite axis (ZStack /
/// Hug parents propagate `f32::INFINITY`).
pub(crate) type AvailableKey = (i32, i32);

/// Triple identifying *which* snapshot a measure call should match
/// against. Built once at the top of a measure call and reused for
/// the cache write-back at the bottom.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubtreeCacheKey {
    pub wid: WidgetId,
    pub subtree_hash: NodeHash,
    pub available_q: AvailableKey,
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
    (quantize_axis(s.w), quantize_axis(s.h))
}

/// Compaction trigger: arena length must exceed `live_entries × this`.
/// `2.0` keeps fragmentation under one extra arena's worth without
/// firing compactions on every miss frame.
const COMPACT_RATIO: u32 = 2;
/// Don't bother compacting until live data is at least this many
/// entries — avoids compaction spam at warmup when the arena is tiny.
const COMPACT_FLOOR: u32 = 64;

#[derive(Default)]
pub(crate) struct MeasureCache {
    /// Backing storage for every snapshot's `desired` data. Live
    /// regions are pointed at by `snapshots`; freed regions sit as
    /// garbage until the next [`Self::compact`].
    pub desired_arena: Vec<Size>,
    /// Parallel to `desired_arena`. Same indexing.
    pub text_arena: Vec<Option<ShapedText>>,
    /// Per-`WidgetId` snapshot index. Each value points at a range in
    /// the two arenas above.
    pub snapshots: FxHashMap<WidgetId, ArenaSnapshot>,
    /// Sum of `snap.len` across `snapshots` — the total live data in
    /// the arenas. Garbage = `desired_arena.len() - live_entries`.
    pub live_entries: u32,
}

impl MeasureCache {
    /// Validate the cache for `wid` against the current frame's
    /// `(subtree_hash, available_q)`. On hit, return the arena range
    /// to copy from — usable directly as a slice index into
    /// `desired_arena` / `text_arena`. On miss, `None`.
    #[inline]
    pub fn lookup(
        &self,
        wid: WidgetId,
        curr_hash: NodeHash,
        curr_avail: AvailableKey,
    ) -> Option<Range<usize>> {
        let snap = self.snapshots.get(&wid)?;
        if snap.subtree_hash != curr_hash {
            return None;
        }
        if snap.available_q != curr_avail {
            return None;
        }
        let start = snap.start as usize;
        Some(start..start + snap.len as usize)
    }

    /// Overwrite (or insert) `wid`'s snapshot. Hot path is in-place
    /// memcpy when the existing range has the same length —
    /// expected to hit ~always once a widget reaches steady state,
    /// since `subtree_hash` includes structure (same hash → same
    /// subtree size). Size mismatches mark the old range as garbage
    /// and append a fresh range to the arena.
    pub fn write_subtree(
        &mut self,
        key: SubtreeCacheKey,
        desired: &[Size],
        text_shapes: &[Option<ShapedText>],
    ) {
        debug_assert_eq!(desired.len(), text_shapes.len());
        let new_len = desired.len() as u32;

        if let Some(prev) = self.snapshots.get_mut(&key.wid)
            && prev.len == new_len
        {
            // In-place: hot path.
            let s = prev.start as usize;
            let e = s + new_len as usize;
            self.desired_arena[s..e].copy_from_slice(desired);
            self.text_arena[s..e].copy_from_slice(text_shapes);
            prev.subtree_hash = key.subtree_hash;
            prev.available_q = key.available_q;
            return;
        }

        // Different len (or first write): mark any existing range as
        // garbage, append the new one. Subtree size only changes when
        // a widget's structure changes, so this path is rare.
        if let Some(prev) = self.snapshots.get(&key.wid) {
            self.live_entries -= prev.len;
        }
        let start = self.desired_arena.len() as u32;
        self.desired_arena.extend_from_slice(desired);
        self.text_arena.extend_from_slice(text_shapes);
        self.live_entries += new_len;
        self.snapshots.insert(
            key.wid,
            ArenaSnapshot {
                subtree_hash: key.subtree_hash,
                available_q: key.available_q,
                start,
                len: new_len,
            },
        );

        if self.desired_arena.len() as u32 > self.live_entries.saturating_mul(COMPACT_RATIO)
            && self.live_entries > COMPACT_FLOOR
        {
            self.compact();
        }
    }

    /// Drop snapshots for widgets that vanished this frame. The
    /// arena slots they referenced become garbage; a future
    /// `write_subtree` will compact them out once fragmentation
    /// crosses the threshold.
    pub fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.live_entries -= snap.len;
            }
        }
    }

    /// Drop every cross-frame snapshot. Public via
    /// `Ui::__clear_measure_cache` for benchmarks.
    pub fn clear(&mut self) {
        self.desired_arena.clear();
        self.text_arena.clear();
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
            snapshots,
            live_entries,
        } = self;
        let cap = *live_entries as usize;
        let mut new_desired: Vec<Size> = Vec::with_capacity(cap);
        let mut new_text: Vec<Option<ShapedText>> = Vec::with_capacity(cap);
        for snap in snapshots.values_mut() {
            let s = snap.start as usize;
            let e = s + snap.len as usize;
            snap.start = new_desired.len() as u32;
            new_desired.extend_from_slice(&desired_arena[s..e]);
            new_text.extend_from_slice(&text_arena[s..e]);
        }
        *desired_arena = new_desired;
        *text_arena = new_text;
    }
}

#[cfg(test)]
mod tests;
