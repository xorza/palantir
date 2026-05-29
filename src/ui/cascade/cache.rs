//! Cross-frame cascade subtree-skip cache.
//!
//! Mirrors `MeasureCache`'s keying — `(WidgetId, subtree_hash,
//! parent_prefix, root_rect_q)` — for cascade output. On hit, blits
//! the cached per-node rows (`Cascade`, `subtree_paint_rect`,
//! `EntryRow`, paint `Span`) and per-paint rows (`Paint`,
//! `shape_to_paint` links) into the current frame's cascade arenas,
//! skipping the entire subtree's walk.
//!
//! See `docs/roadmap/cascade-cache.md` for motivation and the probe
//! evidence (≥99% steady-state coverage on cached / partial workloads)
//! that drove the implementation.
//!
//! Storage discipline mirrors `MeasureCache`: every per-node
//! parallel array (`subtree_paint_rects`, `entry_rows`, `paint_spans`)
//! rides on `rows`'s [`LiveArena`] counter — same length,
//! acquired/released in lockstep. Per-paint data lives in its own
//! `LiveArena`. Snapshots evict via `sweep_removed`; release marks
//! slots garbage in place and `sweep_removed` then repacks via
//! `compact` once garbage dominates (same trigger as `MeasureCache`).

use crate::common::live_arena::LiveArena;
use crate::forest::rollups::NodeHash;
use crate::forest::seen_ids::WidgetIdMap;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascade, EntryRow, LayerCascades, Paint};
use rustc_hash::FxHashSet;
use soa_rs::Soa;

/// Cache key. Same fields as the prior instrumentation probe, now
/// carrying validity rather than only counting potential hits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ProbeKey {
    pub(crate) subtree_hash: NodeHash,
    pub(crate) parent_prefix: u64,
    /// `layout_rect * 64`, rounded — matches `MeasureCache`'s
    /// `available_q` quantization.
    pub(crate) rect_q: [i32; 4],
}

/// `layout_rect * 64`, rounded — matches `MeasureCache`'s `available_q`.
#[inline]
pub(crate) fn quantize_rect(r: Rect) -> [i32; 4] {
    [
        (r.min.x * 64.0).round() as i32,
        (r.min.y * 64.0).round() as i32,
        (r.size.w * 64.0).round() as i32,
        (r.size.h * 64.0).round() as i32,
    ]
}

#[derive(Clone, Copy)]
struct Snapshot {
    key: ProbeKey,
    /// Slice in per-node arenas (`rows`, `subtree_paint_rects`,
    /// `entry_rows`, `paint_spans`). Length equals the subtree's node
    /// count. The root's `subtree_paint_rect` (folded into the parent
    /// stack on hit) is `subtree_paint_rects[nodes.start]` — read at
    /// blit time, no field.
    nodes: Span,
    /// Slice in `paints` arena.
    paints: Span,
    /// Set by `blit`, cleared by `capture` — was this snapshot served at
    /// least once since it was last (re)captured? Drives the thrash gate:
    /// a snapshot re-captured without ever being blitted was pure copy
    /// cost.
    blitted: bool,
    /// Consecutive `capture` attempts for this `wid` whose prior snapshot
    /// was never blitted. Grows while a subtree's key shifts every frame
    /// (resize / scroll); resets to 0 the moment a blit lands. See
    /// [`thrash_decision`].
    cold_streak: u16,
}

/// Wasted captures tolerated before the thrash gate starts skipping. Two
/// gives a one-off miss (a content change that then stabilizes) room to
/// re-establish its snapshot before we conclude the key is thrashing.
const THRASH_STREAK: u16 = 2;
/// Once thrashing, capture only one in this many attempts. The skipped
/// frames pay nothing; the periodic capture is a self-heal probe — if the
/// workload has stabilized, the next frame blits it and the streak resets
/// to full-rate capture. 8 → ~7/8 of the wasted copy cost removed during a
/// sustained drag, recovery within ≤8 frames of stabilizing.
const BACKOFF_PERIOD: u16 = 8;

/// Outcome of the per-`wid` thrash gate.
struct ThrashDecision {
    /// Whether to actually write the capture this frame.
    capture: bool,
    /// The `cold_streak` to store on the (kept or rewritten) snapshot.
    cold_streak: u16,
}

/// Decide whether to capture `wid` this frame given its prior snapshot's
/// `(blitted_since_capture, cold_streak)` — `None` when no snapshot
/// exists. A subtree whose key shifts every frame is captured but never
/// blitted (resize / scroll): pure copy cost. Once the cold streak
/// crosses [`THRASH_STREAK`] we capture only one in [`BACKOFF_PERIOD`]
/// frames. It self-heals: a prior snapshot that *was* blitted resets the
/// streak (the cache is paying off → keep capturing at full rate), and a
/// periodic self-heal capture that the next frame blits does the same.
fn thrash_decision(prior_blitted: Option<bool>, prior_streak: u16) -> ThrashDecision {
    // Cold only when the prior snapshot existed and went un-blitted; a
    // missing or blitted prior means the cache is (or could be) earning
    // its keep, so reset to full-rate capture.
    let cold_streak = if prior_blitted == Some(false) {
        prior_streak.saturating_add(1)
    } else {
        0
    };
    let capture = cold_streak < THRASH_STREAK || cold_streak.is_multiple_of(BACKOFF_PERIOD);
    ThrashDecision {
        capture,
        cold_streak,
    }
}

/// Floor on the size of a cacheable subtree. The bench shows one
/// root-ish subtree (~820 nodes on a ~840-node tree) carries every
/// useful hit; mid-tree ancestors (30–500 nodes) were captured at
/// lower thresholds but never amortized their write cost. 512 keeps
/// only the root-ish subtrees in play; tune down if a workload
/// surfaces a beneficial mid-size cacheable subtree.
const MIN_CACHEABLE_SPAN: u32 = 512;

/// One live snapshot's arena spans, collected into
/// [`CascadeCache::compact_scratch`] and sorted by `nodes.start` to
/// drive the in-place repack.
#[derive(Clone, Copy, Debug)]
struct CompactEntry {
    wid: WidgetId,
    nodes: Span,
    paints: Span,
}

#[derive(Default)]
pub struct CascadeCache {
    snapshots: WidgetIdMap<Snapshot>,
    /// Per-node arenas. `rows` owns the live counter (acquired and
    /// released for `span` items per snapshot); `subtree_paint_rects`,
    /// `entry_rows`, and `paint_spans` are parallel `Vec`s of identical
    /// length.
    rows: LiveArena<Cascade>,
    subtree_paint_rects: Vec<Rect>,
    /// Per-node snapshot of `EntryRow`. Named `entry_rows` (not
    /// `entries`) so it doesn't shadow the `entries: &Soa<EntryRow>`
    /// parameter on `blit` / `capture` — that parameter is the *live*
    /// walk's hit-test SoA, distinct from this snapshot vec.
    entry_rows: Vec<EntryRow>,
    /// `node_spans`, stored with `start` relative to the subtree's
    /// paint base — rebased on blit.
    paint_spans: Vec<Span>,
    paints: LiveArena<Paint>,
    /// Reusable scratch for in-place [`Self::compact`]: one
    /// [`CompactEntry`] per live snapshot, sorted by `nodes.start` so
    /// the repack packs front-to-back without a snapshot clobbering one
    /// it hasn't moved yet. Retained across frames (capacity reused) so
    /// compaction allocates nothing.
    compact_scratch: Vec<CompactEntry>,
    /// Stats for the most recent `CascadesEngine::run`. Reset at the
    /// top of each run. Gated behind `internals` so production builds
    /// don't carry per-blit / per-capture increments.
    #[cfg(any(test, feature = "internals"))]
    pub hits: u32,
    #[cfg(any(test, feature = "internals"))]
    pub misses: u32,
    #[cfg(any(test, feature = "internals"))]
    pub captures: u32,
    /// Capture attempts the thrash gate skipped this frame.
    #[cfg(any(test, feature = "internals"))]
    pub skips: u32,
    #[cfg(any(test, feature = "internals"))]
    pub nodes_blit: u32,
}

impl CascadeCache {
    pub(crate) fn reset_counters(&mut self) {
        #[cfg(any(test, feature = "internals"))]
        {
            self.hits = 0;
            self.misses = 0;
            self.captures = 0;
            self.skips = 0;
            self.nodes_blit = 0;
        }
    }

    #[inline]
    pub(crate) fn is_cacheable(span: u32) -> bool {
        span >= MIN_CACHEABLE_SPAN
    }

    #[inline]
    pub(crate) fn probe(&self, wid: WidgetId, key: &ProbeKey) -> bool {
        self.snapshots
            .get(&wid)
            .is_some_and(|snap| snap.key == *key)
    }

    /// Blit a cached subtree into the live cascade output. Caller must
    /// have already verified the key matches via `probe`. Returns the
    /// root's `subtree_paint_rect` — caller folds it into the parent
    /// stack frame's running union and advances the walk cursor to
    /// `subtree_end`.
    pub(crate) fn blit(
        &mut self,
        wid: WidgetId,
        root_idx: u32,
        subtree_end: u32,
        cascades: &mut LayerCascades,
        entries: &mut Soa<EntryRow>,
    ) -> Rect {
        // The snapshot pays off this frame: mark it served and reset the
        // thrash streak (so `capture` keeps refreshing it at full rate)
        // while we hold the lookup, then copy it out for the blit below.
        let snap = {
            let s = self
                .snapshots
                .get_mut(&wid)
                .expect("blit called without a successful probe");
            s.blitted = true;
            s.cold_streak = 0;
            *s
        };
        let span = subtree_end - root_idx;
        // Release asserts (not debug): a mis-sized blit or cursor
        // misalignment silently corrupts the `entries_base + node.0`
        // mapping every downstream hit-test / damage read trusts, and
        // both checks are a single O(1) compare.
        assert_eq!(span, snap.nodes.len, "snapshot node count drift");
        assert_eq!(
            cascades.rows.len() as u32,
            root_idx,
            "blit must align with the live cascade's per-node arena cursor",
        );
        let nodes = snap.nodes.range();
        cascades
            .rows
            .extend_from_slice(&self.rows.items[nodes.clone()]);
        cascades
            .subtree_paint_rects
            .extend_from_slice(&self.subtree_paint_rects[nodes.clone()]);
        entries.extend(self.entry_rows[nodes.clone()].iter().copied());
        let paint_base = cascades.paint_arena.rows.len() as u32;
        cascades
            .paint_arena
            .rows
            .extend_from_slice(&self.paints.items[snap.paints.range()]);
        for (offset, ps) in self.paint_spans[nodes].iter().enumerate() {
            cascades.paint_arena.node_spans[(root_idx as usize) + offset] =
                Span::new(paint_base + ps.start, ps.len);
        }
        #[cfg(any(test, feature = "internals"))]
        {
            self.hits += 1;
            self.nodes_blit += span;
        }
        self.subtree_paint_rects[snap.nodes.start as usize]
    }

    /// Capture a freshly-walked subtree. Called from `run_tree`'s pop
    /// loop when a Frame whose subtree was missed (or never probed)
    /// completes. No-op when `span < MIN_CACHEABLE_SPAN`.
    ///
    /// In-place rewrite path: when an existing snapshot for `wid` has
    /// the same node + paint counts as the new capture, overwrite its
    /// arena slots rather than evict-and-append. Without this, an
    /// animated widget whose authoring hash shifts every frame would
    /// grow the arenas monotonically and violate the alloc-free
    /// invariant (`alloc_free` test pins zero blocks in steady state).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture(
        &mut self,
        wid: WidgetId,
        key: ProbeKey,
        root_idx: u32,
        subtree_end: u32,
        cascades: &LayerCascades,
        entries: &Soa<EntryRow>,
        entries_base: u32,
        paint_capture_start: u32,
    ) {
        let span = subtree_end - root_idx;
        if !Self::is_cacheable(span) {
            return;
        }

        // Thrash gate: a subtree whose key shifts every frame (resize /
        // scroll) is captured but never blitted — pure copy cost. Back
        // off to one capture per `BACKOFF_PERIOD` once it's clearly
        // thrashing, keeping the (stale) snapshot so the streak survives.
        let decision = match self.snapshots.get(&wid) {
            Some(s) => thrash_decision(Some(s.blitted), s.cold_streak),
            None => thrash_decision(None, 0),
        };
        if !decision.capture {
            // A skip implies a prior un-blitted snapshot (the streak can't
            // grow otherwise), so the entry is always present here.
            self.snapshots
                .get_mut(&wid)
                .expect("skip implies a prior un-blitted snapshot")
                .cold_streak = decision.cold_streak;
            #[cfg(any(test, feature = "internals"))]
            {
                self.skips += 1;
            }
            return;
        }

        let lo = root_idx as usize;
        let hi = subtree_end as usize;
        let paint_capture_end = cascades.paint_arena.rows.len() as u32;
        let paints_len = paint_capture_end - paint_capture_start;
        let src_paints =
            &cascades.paint_arena.rows[paint_capture_start as usize..paint_capture_end as usize];
        let node_spans = &cascades.paint_arena.node_spans;

        let e_wid = entries.widget_id();
        let e_rect = entries.rect();
        let e_sense = entries.sense();
        let e_focus = entries.focusable();
        let e_dis = entries.disabled();
        let e_layout = entries.layout_rect();
        // One row of the hit-test snapshot, built from the live walk's
        // SoA at global entry index `gi`. Same construction on both the
        // reuse and append paths.
        let entry_at = |gi: usize| EntryRow {
            widget_id: e_wid[gi],
            rect: e_rect[gi],
            sense: e_sense[gi],
            focusable: e_focus[gi],
            disabled: e_dis[gi],
            layout_rect: e_layout[gi],
        };
        // Rebase a live per-node paint span to subtree-local form (start
        // relative to this capture's paint base); empty spans pin to 0.
        let rebase = |s: Span| {
            let start = if s.len == 0 {
                0
            } else {
                s.start - paint_capture_start
            };
            Span::new(start, s.len)
        };

        // Decide in-place reuse vs evict-and-append. The reuse predicate
        // matches on shape (length pair) — same widget, same per-frame
        // footprint. The key itself differs (that's why we're capturing
        // instead of hitting), but the out-arena offsets remain valid.
        let reuse = self
            .snapshots
            .get(&wid)
            .copied()
            .filter(|old| old.nodes.len == span && old.paints.len == paints_len);

        let (nodes_start, paints_start) = if let Some(old) = reuse {
            let nb = old.nodes.start as usize;
            self.rows.items[nb..nb + span as usize].copy_from_slice(&cascades.rows[lo..hi]);
            self.subtree_paint_rects[nb..nb + span as usize]
                .copy_from_slice(&cascades.subtree_paint_rects[lo..hi]);
            for off in 0..span as usize {
                self.entry_rows[nb + off] = entry_at(entries_base as usize + lo + off);
                self.paint_spans[nb + off] = rebase(node_spans[lo + off]);
            }
            let pb = old.paints.start as usize;
            self.paints.items[pb..pb + paints_len as usize].copy_from_slice(src_paints);
            (old.nodes.start, old.paints.start)
        } else {
            if let Some(old) = self.snapshots.remove(&wid) {
                self.release(old);
            }
            let nodes_start = self.rows.items.len() as u32;
            let paints_start = self.paints.items.len() as u32;
            self.rows.items.extend_from_slice(&cascades.rows[lo..hi]);
            self.subtree_paint_rects
                .extend_from_slice(&cascades.subtree_paint_rects[lo..hi]);
            for (off, &s) in node_spans[lo..hi].iter().enumerate() {
                self.entry_rows
                    .push(entry_at(entries_base as usize + lo + off));
                self.paint_spans.push(rebase(s));
            }
            self.paints.items.extend_from_slice(src_paints);
            self.rows.acquire(span);
            self.paints.acquire(paints_len);
            (nodes_start, paints_start)
        };

        self.snapshots.insert(
            wid,
            Snapshot {
                key,
                nodes: Span::new(nodes_start, span),
                paints: Span::new(paints_start, paints_len),
                blitted: false,
                cold_streak: decision.cold_streak,
            },
        );
        #[cfg(any(test, feature = "internals"))]
        {
            self.captures += 1;
        }
    }

    fn release(&mut self, snap: Snapshot) {
        self.rows.release(snap.nodes.len);
        self.paints.release(snap.paints.len);
    }

    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.release(snap);
            }
        }
        self.maybe_compact();
    }

    /// Repack the arenas when garbage from released / length-changed
    /// snapshots dominates. Without it a cacheable subtree whose node or
    /// paint *count* shifts every frame (the reuse-in-place path only
    /// covers same-length churn) would grow `rows`/`paints` unbounded —
    /// the same failure `MeasureCache` compacts to avoid. Called from
    /// `sweep_removed` only: `acquire` grows `items` and `live` in
    /// lockstep, so writes can't trip the ratio — only releases can.
    fn maybe_compact(&mut self) {
        if self.rows.needs_compact() || self.paints.needs_compact() {
            self.compact();
        }
    }

    /// Repack the arenas **in place**, sliding each live snapshot's
    /// node and paint ranges down over the garbage left by released or
    /// length-changed snapshots, then truncating the dead tail. O(live)
    /// `copy_within` and no allocation — the backing `Vec`s keep their
    /// capacity (`truncate` doesn't shrink). The per-node parallel `Vec`s
    /// (`subtree_paint_rects`, `entry_rows`, `paint_spans`) ride on
    /// `rows`'s span, so they slide with the same range.
    ///
    /// Snapshots are processed in ascending `nodes.start` order so the
    /// write cursor never overtakes an unmoved snapshot: the cumulative
    /// live length of all earlier snapshots is `<=` the current
    /// snapshot's original start, so each `copy_within` writes to an
    /// offset `<=` its source (a safe overlapping move toward the front).
    /// Node and paint arenas share that order — both are appended (and
    /// repacked) in lockstep — so a snapshot earlier in node order is
    /// earlier in paint order too, and `paint_w <= snap.paints.start`
    /// holds by the same argument.
    ///
    /// (The earlier implementation rebuilt all five arenas with
    /// `Vec::with_capacity`. On a resize / text-reflow workload this
    /// fires roughly every frame — `CascadeCache::sweep_removed` was the
    /// single largest allocator in `alloc_resize`'s dhat dump — so the
    /// fresh-`Vec` rebuild meant ~1 MB/frame of churn the in-place form
    /// removes entirely.)
    fn compact(&mut self) {
        let mut scratch = std::mem::take(&mut self.compact_scratch);
        scratch.clear();
        scratch.extend(self.snapshots.iter().map(|(wid, snap)| CompactEntry {
            wid: *wid,
            nodes: snap.nodes,
            paints: snap.paints,
        }));
        scratch.sort_unstable_by_key(|e| e.nodes.start);

        let mut node_w = 0u32;
        let mut paint_w = 0u32;
        for e in scratch.iter() {
            if e.nodes.start != node_w {
                let src = e.nodes.range();
                let dst = node_w as usize;
                self.rows.items.copy_within(src.clone(), dst);
                self.subtree_paint_rects.copy_within(src.clone(), dst);
                self.entry_rows.copy_within(src.clone(), dst);
                self.paint_spans.copy_within(src, dst);
            }
            if e.paints.start != paint_w {
                self.paints
                    .items
                    .copy_within(e.paints.range(), paint_w as usize);
            }
            // Rewrite the snapshot's offsets to match the slid position,
            // then advance the write cursors.
            let snap = self
                .snapshots
                .get_mut(&e.wid)
                .expect("snapshot present: scratch built from snapshots this call");
            snap.nodes = Span::new(node_w, e.nodes.len);
            snap.paints = Span::new(paint_w, e.paints.len);
            node_w += e.nodes.len;
            paint_w += e.paints.len;
        }

        // `live` is unchanged — only the garbage tail is dropped, so the
        // repacked `items.len()` now equals `live`.
        self.rows.items.truncate(node_w as usize);
        self.subtree_paint_rects.truncate(node_w as usize);
        self.entry_rows.truncate(node_w as usize);
        self.paint_spans.truncate(node_w as usize);
        self.paints.items.truncate(paint_w as usize);

        self.compact_scratch = scratch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forest::rollups::CascadeInputHash;
    use crate::input::sense::Sense;

    /// Append a snapshot for `wid` directly into the arenas, tagging
    /// every row with `tag * 1000 + local_offset` so the slid position
    /// is verifiable after compaction.
    fn push_snapshot(cache: &mut CascadeCache, wid: WidgetId, tag: u64, nodes: u32, paints: u32) {
        let nodes_start = cache.rows.items.len() as u32;
        let paints_start = cache.paints.items.len() as u32;
        for off in 0..nodes {
            let id = tag * 1000 + off as u64;
            cache.rows.items.push(Cascade {
                paint_rect: Rect::new(id as f32, 0.0, 0.0, 0.0),
                cascade_input: CascadeInputHash(id),
            });
            cache
                .subtree_paint_rects
                .push(Rect::new(id as f32, 0.0, 0.0, 0.0));
            cache.entry_rows.push(EntryRow {
                widget_id: WidgetId::from_hash(id),
                rect: Rect::ZERO,
                sense: Sense::NONE,
                focusable: false,
                disabled: false,
                layout_rect: Rect::ZERO,
            });
            cache.paint_spans.push(Span::new(id as u32, 0));
        }
        for off in 0..paints {
            cache.paints.items.push(Paint {
                screen: Rect::ZERO,
                hash: NodeHash(tag * 1000 + off as u64),
            });
        }
        cache.rows.acquire(nodes);
        cache.paints.acquire(paints);
        cache.snapshots.insert(
            wid,
            Snapshot {
                key: ProbeKey {
                    subtree_hash: NodeHash(tag),
                    parent_prefix: 0,
                    rect_q: [0; 4],
                },
                nodes: Span::new(nodes_start, nodes),
                paints: Span::new(paints_start, paints),
                blitted: false,
                cold_streak: 0,
            },
        );
    }

    /// `compact()` repacks **in place**: it slides surviving snapshots
    /// down over a released snapshot's garbage with `copy_within` +
    /// `truncate`, never reallocating. We lay down three snapshots
    /// A/B/C, release the middle one, and assert (1) the backing `Vec`s
    /// keep their pre-compact capacity and pointer — the pre-fix code
    /// rebuilt into `Vec::with_capacity(live)`, which would shrink
    /// capacity and move the pointer — and (2) the surviving rows carry
    /// their original tags at the slid offsets, with snapshot spans
    /// rewritten to match.
    #[test]
    fn compact_repacks_in_place_without_reallocating() {
        let (a, b, c) = (
            WidgetId::from_hash("a"),
            WidgetId::from_hash("b"),
            WidgetId::from_hash("c"),
        );
        let mut cache = CascadeCache::default();
        push_snapshot(&mut cache, a, 1, 2, 1);
        push_snapshot(&mut cache, b, 2, 3, 2);
        push_snapshot(&mut cache, c, 3, 2, 1);
        assert_eq!(cache.rows.items.len(), 7);
        assert_eq!(cache.paints.items.len(), 4);

        // Release the middle snapshot — its slots become garbage that
        // compaction must slide C over.
        let removed = cache.snapshots.remove(&b).unwrap();
        cache.release(removed);

        let rows_cap = cache.rows.items.capacity();
        let rows_ptr = cache.rows.items.as_ptr();
        let paints_cap = cache.paints.items.capacity();
        let paints_ptr = cache.paints.items.as_ptr();

        cache.compact();

        // Garbage tail dropped; len now equals live.
        assert_eq!(cache.rows.items.len(), 4, "A(2) + C(2) survive");
        assert_eq!(cache.paints.items.len(), 2, "A(1) + C(1) survive");
        assert_eq!(cache.rows.live, 4);
        assert_eq!(cache.paints.live, 2);

        // In place: capacity and backing pointer are untouched (a
        // rebuild would shrink capacity to `live` and move the pointer).
        assert_eq!(
            cache.rows.items.capacity(),
            rows_cap,
            "node arena reallocated — compact must be in place",
        );
        assert_eq!(
            cache.rows.items.as_ptr(),
            rows_ptr,
            "node backing pointer moved"
        );
        assert_eq!(
            cache.paints.items.capacity(),
            paints_cap,
            "paint arena reallocated"
        );
        assert_eq!(
            cache.paints.items.as_ptr(),
            paints_ptr,
            "paint backing pointer moved"
        );

        // A stayed put; C slid from start 4 → 2 (nodes) and 3 → 1 (paints).
        let snap_a = cache.snapshots[&a];
        let snap_c = cache.snapshots[&c];
        assert_eq!(snap_a.nodes, Span::new(0, 2));
        assert_eq!(snap_a.paints, Span::new(0, 1));
        assert_eq!(
            snap_c.nodes,
            Span::new(2, 2),
            "C slid down over B's garbage"
        );
        assert_eq!(snap_c.paints, Span::new(1, 1));

        // Data integrity: every surviving row carries its original tag at
        // the slid offset (A: tag 1, C: tag 3; cascade_input = tag*1000+off).
        let tag_of = |i: usize| cache.rows.items[i].cascade_input.0;
        assert_eq!([tag_of(0), tag_of(1)], [1000, 1001], "A intact at front");
        assert_eq!([tag_of(2), tag_of(3)], [3000, 3001], "C slid intact");
        // A parallel per-node column must slide on `rows`'s range in
        // lockstep — `subtree_paint_rects[i].min.x` carries the same tag.
        let rect_tag = |i: usize| cache.subtree_paint_rects[i].min.x;
        assert_eq!(
            [rect_tag(0), rect_tag(1), rect_tag(2), rect_tag(3)],
            [1000.0, 1001.0, 3000.0, 3001.0],
            "subtree_paint_rects slid in lockstep with rows",
        );
        assert_eq!(
            cache.paints.items[0].hash,
            NodeHash(1000),
            "A's paint intact"
        );
        assert_eq!(
            cache.paints.items[1].hash,
            NodeHash(3000),
            "C's paint slid intact"
        );
    }

    /// Pins the pure thrash-gate logic: a missing or blitted prior keeps
    /// full-rate capture; an un-blitted prior grows the streak and, past
    /// `THRASH_STREAK`, captures only on `BACKOFF_PERIOD` boundaries.
    #[test]
    fn thrash_decision_table() {
        // No prior snapshot → capture at full rate, streak 0.
        let d = thrash_decision(None, 0);
        assert!(d.capture && d.cold_streak == 0, "no prior → capture");

        // Prior was blitted (cache earning its keep) → reset + capture,
        // regardless of how cold it previously was.
        let d = thrash_decision(Some(true), 99);
        assert!(d.capture && d.cold_streak == 0, "blitted prior resets");

        // First wasted capture: streak 0→1, still below threshold → capture.
        let d = thrash_decision(Some(false), 0);
        assert!(d.capture && d.cold_streak == 1);

        // Reaching THRASH_STREAK flips to back-off (skip).
        let d = thrash_decision(Some(false), THRASH_STREAK - 1);
        assert_eq!(d.cold_streak, THRASH_STREAK);
        assert!(!d.capture, "at the streak threshold the gate skips");

        // Every streak between the threshold and the next period boundary
        // skips...
        for prior in (THRASH_STREAK - 1)..(BACKOFF_PERIOD - 1) {
            let d = thrash_decision(Some(false), prior);
            assert!(!d.capture, "streak {} should skip", d.cold_streak);
        }
        // ...and the boundary captures (the self-heal probe).
        let d = thrash_decision(Some(false), BACKOFF_PERIOD - 1);
        assert_eq!(d.cold_streak, BACKOFF_PERIOD);
        assert!(d.capture, "periodic self-heal capture lands");
    }

    /// End-to-end: a >`MIN_CACHEABLE_SPAN` subtree whose `rect_q` shifts
    /// every frame (rotating surface size) is captured but never blitted,
    /// so the gate must engage (`skips > 0`). The identical fixture held
    /// at a *stable* size must blit instead — proving the gate never backs
    /// off a cache that's paying off (`skips == 0`, `hits > 0`).
    #[test]
    fn capture_throttles_under_resize_thrash_but_not_when_stable() {
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::widgets::frame::Frame;
        use crate::widgets::panel::Panel;
        use glam::UVec2;

        // Panel fills the surface width so its arranged rect — and thus
        // its cascade key — shifts with every surface resize. (A hugging
        // panel would be width-independent and hit the cache forever,
        // exercising no thrash.)
        let build = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("big"))
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    for i in 0..(MIN_CACHEABLE_SPAN + 16) {
                        Frame::new()
                            .id(WidgetId::from_hash(("f", i)))
                            .size(4.0)
                            .show(ui);
                    }
                });
        };

        // Continuous drag: a strictly-increasing width every frame so the
        // subtree key never re-matches a stale snapshot (no hits possible).
        // Counters reset per run, so accumulate across frames.
        let frames = 60u32;
        let mut ui = Ui::for_test();
        let (mut total_caps, mut total_skips, mut total_hits) = (0u32, 0u32, 0u32);
        for f in 0..frames {
            ui.run_at_acked(UVec2::new(400 + f * 3, 400), &mut |ui: &mut Ui| build(ui));
            total_caps += ui.cascades_engine.cache.captures;
            total_skips += ui.cascades_engine.cache.skips;
            total_hits += ui.cascades_engine.cache.hits;
        }
        assert!(
            total_skips > 0,
            "continuous drag must engage the gate (skips={total_skips})",
        );
        assert!(
            total_caps < frames,
            "captures must be throttled well below one-per-frame \
             (captures={total_caps}, frames={frames}, skips={total_skips}, hits={total_hits})",
        );

        // Stable: same size every frame → the subtree blits after warmup,
        // so the gate must never engage.
        let mut stable = Ui::for_test();
        for _ in 0..40 {
            stable.run_at_acked(UVec2::new(400, 400), &mut |ui: &mut Ui| build(ui));
        }
        assert_eq!(
            stable.cascades_engine.cache.skips, 0,
            "stable workload must not back off",
        );
        assert!(
            stable.cascades_engine.cache.hits > 0,
            "stable workload must serve from the cache",
        );
    }
}
