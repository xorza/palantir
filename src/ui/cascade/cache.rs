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

use crate::common::cache_arena::LiveArena;
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
}

/// Floor on the size of a cacheable subtree. The bench shows one
/// root-ish subtree (~820 nodes on a ~840-node tree) carries every
/// useful hit; mid-tree ancestors (30–500 nodes) were captured at
/// lower thresholds but never amortized their write cost. 512 keeps
/// only the root-ish subtrees in play; tune down if a workload
/// surfaces a beneficial mid-size cacheable subtree.
const MIN_CACHEABLE_SPAN: u32 = 512;

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
    /// Stats for the most recent `CascadesEngine::run`. Reset at the
    /// top of each run. Gated behind `internals` so production builds
    /// don't carry per-blit / per-capture increments.
    #[cfg(any(test, feature = "internals"))]
    pub hits: u32,
    #[cfg(any(test, feature = "internals"))]
    pub misses: u32,
    #[cfg(any(test, feature = "internals"))]
    pub captures: u32,
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
        let snap = *self
            .snapshots
            .get(&wid)
            .expect("blit called without a successful probe");
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

    /// Walk every snapshot, copy its live node + paint ranges into
    /// freshly-packed arenas, and rewrite the snapshot offsets. O(live);
    /// mirrors `MeasureCache::compact`. The per-node parallel `Vec`s
    /// (`subtree_paint_rects`, `entry_rows`, `paint_spans`) ride on
    /// `rows`'s span, so they repack with the same range.
    fn compact(&mut self) {
        let mut new_rows: Vec<Cascade> = Vec::with_capacity(self.rows.live);
        let mut new_subtree_paint_rects: Vec<Rect> = Vec::with_capacity(self.rows.live);
        let mut new_entry_rows: Vec<EntryRow> = Vec::with_capacity(self.rows.live);
        let mut new_paint_spans: Vec<Span> = Vec::with_capacity(self.rows.live);
        let mut new_paints: Vec<Paint> = Vec::with_capacity(self.paints.live);
        for snap in self.snapshots.values_mut() {
            let nodes = snap.nodes.range();
            snap.nodes.start = new_rows.len() as u32;
            new_rows.extend_from_slice(&self.rows.items[nodes.clone()]);
            new_subtree_paint_rects.extend_from_slice(&self.subtree_paint_rects[nodes.clone()]);
            new_entry_rows.extend_from_slice(&self.entry_rows[nodes.clone()]);
            new_paint_spans.extend_from_slice(&self.paint_spans[nodes]);
            let paints = snap.paints.range();
            snap.paints.start = new_paints.len() as u32;
            new_paints.extend_from_slice(&self.paints.items[paints]);
        }
        // `live` is unchanged — only the garbage tail is dropped, so the
        // repacked `items.len()` now equals `live`.
        self.rows.items = new_rows;
        self.subtree_paint_rects = new_subtree_paint_rects;
        self.entry_rows = new_entry_rows;
        self.paint_spans = new_paint_spans;
        self.paints.items = new_paints;
    }
}
