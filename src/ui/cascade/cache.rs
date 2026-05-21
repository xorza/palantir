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
//! parallel array (`sptrs`, `entries`, `paint_spans`) rides on
//! `rows`'s [`LiveArena`] counter — same length, acquired/released in
//! lockstep. Per-paint and per-shape-link data live in their own
//! `LiveArena`s. Snapshots evict via `sweep_removed`; release marks
//! slots garbage in place (no compaction yet — add when arena bloat
//! shows up).

use crate::common::cache_arena::LiveArena;
use crate::forest::rollups::NodeHash;
use crate::forest::tree::Tree;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::{Cascade, EntryRow, LayerCascades, Paint};
use rustc_hash::{FxHashMap, FxHashSet};
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

/// `shape_to_paint` link in subtree-relative form. `rel_shape_idx` is
/// relative to the subtree's first shape; `rel_paint_idx` is relative
/// to the snapshot's `paints` base. Both rebased on blit.
#[derive(Clone, Copy)]
struct ShapeLink {
    rel_shape_idx: u32,
    rel_paint_idx: u32,
}

#[derive(Clone, Copy)]
struct Snapshot {
    key: ProbeKey,
    /// Slice in per-node arenas (`rows`, `sptrs`, `entries`,
    /// `paint_spans`). Length equals the subtree's node count.
    nodes: Span,
    /// Slice in `paints` arena.
    paints: Span,
    /// Slice in `shape_links` arena.
    shape_links: Span,
    /// `subtree_paint_rect[root]` after the original walk's rollup
    /// completed — fed into the parent stack's running union on hit.
    /// Equal to the first entry of this snapshot's per-node `sptrs`
    /// slice; stashed explicitly so the hit path doesn't need a
    /// second arena read.
    root_paint_rect: Rect,
}

/// Floor on the size of a cacheable subtree. The bench shows one
/// root-ish subtree (~820 nodes on a ~840-node tree) carries every
/// useful hit; mid-tree ancestors (30–500 nodes) were captured at
/// lower thresholds but never amortized their write cost. 512 keeps
/// only the root-ish subtrees in play; tune down if a workload
/// surfaces a beneficial mid-size cacheable subtree.
const MIN_CACHEABLE_SPAN: u32 = 512;

/// Consecutive captures (this wid, key changes every time, no hit in
/// between) that flip the wid into cooldown. Three is enough to ride
/// out a one-off resize or theme swap without tripping, while still
/// reacting fast to a sustained thrash (e.g. continuous window
/// resize, where every iter the cache pays full capture cost but
/// next-iter's `rect_q` invalidates the snapshot).
const CHURN_THRESHOLD: u8 = 3;

/// Frames to skip capture once cooldown engages. Strictly frame-based
/// (driven by `frame_seq`), so cooldown elapses on schedule whether or
/// not the wid keeps getting `capture` calls — a wid that starts
/// hitting mid-cooldown still resumes capture at the same wall-clock
/// frame it would have otherwise.
const COOL_FRAMES: u32 = 64;

/// Per-widget thrash state for the capture-backoff. Held parallel to
/// `snapshots`: a wid in cooldown has *no* snapshot entry (we evicted
/// it when entering cooldown) but does have a churn row tracking the
/// next-key probe. Once a capture call lands with the same key as the
/// previous one, the wid exits cooldown.
#[derive(Clone, Copy)]
struct Churn {
    /// Most recent key seen on a `capture` call for this wid. During
    /// cooldown a match between this and the incoming key means the
    /// thrash has settled.
    last_key: ProbeKey,
    /// Consecutive thrashing captures (key changed each time, no hit
    /// between them). Resets on a hit (detected via `last_capture_frame`
    /// gap) or on an exact key match in cooldown.
    miss_streak: u8,
    /// `frame_seq` at which cooldown ends; `capture` is a no-op while
    /// `frame_seq < cool_until`. Zero means "not cooling".
    cool_until: u32,
    /// `frame_seq` at the last `capture` call for this wid. If the gap
    /// is > 1 frame, this wid hit the snapshot in the meantime —
    /// reset `miss_streak` since the captured snapshot did amortize.
    last_capture_frame: u32,
}

/// Outcome of the churn check at the top of `capture`. Computed in one
/// pass so the snapshot mutation that follows doesn't have to
/// interleave with churn-map borrows.
enum ChurnDecision {
    /// Proceed with capture normally.
    Capture,
    /// Skip this capture; the wid is still cooling.
    Skip,
    /// Skip this capture and evict any existing snapshot — we just
    /// entered cooldown and the prior snapshot is stale.
    SkipAndEvict,
}

#[derive(Default)]
pub struct CascadeCache {
    snapshots: FxHashMap<WidgetId, Snapshot>,
    churn: FxHashMap<WidgetId, Churn>,
    /// Monotonic frame counter, incremented in `reset_counters`. Lets
    /// `Churn::last_capture_frame` detect a "we hit on at least one
    /// intermediate frame" gap without an explicit hit flag.
    frame_seq: u32,
    /// Per-node arenas. `rows` owns the live counter (acquired and
    /// released for `span` items per snapshot); `sptrs`, `entry_rows`,
    /// and `paint_spans` are parallel `Vec`s of identical length.
    rows: LiveArena<Cascade>,
    sptrs: Vec<Rect>,
    /// Per-node snapshot of `EntryRow`. Named `entry_rows` (not
    /// `entries`) so it doesn't shadow the `entries: &Soa<EntryRow>`
    /// parameter on `blit` / `capture` — that parameter is the *live*
    /// walk's hit-test SoA, distinct from this snapshot vec.
    entry_rows: Vec<EntryRow>,
    /// `node_spans`, stored with `start` relative to the subtree's
    /// paint base — rebased on blit.
    paint_spans: Vec<Span>,
    paints: LiveArena<Paint>,
    shape_links: LiveArena<ShapeLink>,
    /// Stats for the most recent `CascadesEngine::run`. Reset at the
    /// top of each run.
    pub hits: u32,
    pub misses: u32,
    pub captures: u32,
    pub nodes_blit: u32,
}

impl CascadeCache {
    pub(crate) fn reset_counters(&mut self) {
        self.hits = 0;
        self.misses = 0;
        self.captures = 0;
        self.nodes_blit = 0;
        self.frame_seq = self.frame_seq.wrapping_add(1);
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blit(
        &mut self,
        wid: WidgetId,
        tree: &Tree,
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
        debug_assert_eq!(span, snap.nodes.len, "snapshot node count drift");
        debug_assert_eq!(
            cascades.rows.len() as u32,
            root_idx,
            "blit must align with the live cascade's per-node arena cursor",
        );
        let n_lo = snap.nodes.start as usize;
        let n_hi = n_lo + snap.nodes.len as usize;
        cascades
            .rows
            .extend_from_slice(&self.rows.items[n_lo..n_hi]);
        cascades
            .subtree_paint_rects
            .extend_from_slice(&self.sptrs[n_lo..n_hi]);
        for entry in &self.entry_rows[n_lo..n_hi] {
            entries.push(*entry);
        }
        let paint_base = cascades.paint_arena.rows.len() as u32;
        let p_lo = snap.paints.start as usize;
        let p_hi = p_lo + snap.paints.len as usize;
        cascades
            .paint_arena
            .rows
            .extend_from_slice(&self.paints.items[p_lo..p_hi]);
        for (offset, ps) in self.paint_spans[n_lo..n_hi].iter().enumerate() {
            cascades.paint_arena.node_spans[(root_idx as usize) + offset] =
                Span::new(paint_base + ps.start, ps.len);
        }
        // Subtree's shape range is contiguous in `tree.shapes.records`,
        // anchored at the root's `shape_span.start`. Stored
        // shape-relative on capture so identical subtrees at different
        // tree positions hit the same snapshot.
        let shape_base = tree.records.shape_span()[root_idx as usize].start;
        let l_lo = snap.shape_links.start as usize;
        let l_hi = l_lo + snap.shape_links.len as usize;
        for link in &self.shape_links.items[l_lo..l_hi] {
            cascades.paint_arena.shape_to_paint[(shape_base + link.rel_shape_idx) as usize] =
                paint_base + link.rel_paint_idx;
        }
        self.hits += 1;
        self.nodes_blit += span;
        snap.root_paint_rect
    }

    /// Capture a freshly-walked subtree. Called from `run_tree`'s pop
    /// loop when a Frame whose subtree was missed (or never probed)
    /// completes. No-op when `span < MIN_CACHEABLE_SPAN`.
    ///
    /// In-place rewrite path: when an existing snapshot for `wid` has
    /// the same node / paint / shape-link counts as the new capture,
    /// overwrite its arena slots rather than evict-and-append. Without
    /// this, an animated widget whose authoring hash shifts every
    /// frame would grow the arenas monotonically and violate the
    /// alloc-free invariant (`alloc_free` test pins zero blocks in
    /// steady state).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture(
        &mut self,
        wid: WidgetId,
        key: ProbeKey,
        tree: &Tree,
        root_idx: u32,
        subtree_end: u32,
        root_paint_rect: Rect,
        cascades: &LayerCascades,
        entries: &Soa<EntryRow>,
        entries_base: u32,
        paint_capture_start: u32,
    ) {
        let span = subtree_end - root_idx;
        if !Self::is_cacheable(span) {
            return;
        }

        // Churn backoff. The resize bench (`frame/resizing_*`) re-walks
        // the same root-ish subtree every iter with a fresh `rect_q`
        // — every capture is paid for, and next-iter's probe always
        // misses. Detect that pattern and stop capturing for a window
        // of frames; resume the moment a capture call lands with the
        // same key as the previous one (signal the thrash settled).
        match self.update_churn(wid, key) {
            ChurnDecision::Skip => return,
            ChurnDecision::SkipAndEvict => {
                if let Some(old) = self.snapshots.remove(&wid) {
                    self.release(old);
                }
                return;
            }
            ChurnDecision::Capture => {}
        }

        let lo = root_idx as usize;
        let hi = subtree_end as usize;
        let paint_capture_end = cascades.paint_arena.rows.len() as u32;
        let paints_len = paint_capture_end - paint_capture_start;

        // Count emitted shape→paint links up front so the in-place
        // path can decide before any writes.
        let shapes_col = tree.records.shape_span();
        let shape_base = shapes_col[root_idx as usize].start;
        let last = shapes_col[(subtree_end - 1) as usize];
        let shape_end = last.start + last.len;
        let shape_to_paint = &cascades.paint_arena.shape_to_paint;
        let mut links_len: u32 = 0;
        for abs_shape in shape_base..shape_end {
            if shape_to_paint[abs_shape as usize] != u32::MAX {
                links_len += 1;
            }
        }

        // Decide in-place reuse vs evict-and-append. The reuse
        // predicate matches on shape (length triple) — same widget,
        // same per-frame footprint. The key itself differs (that's
        // why we're capturing instead of hitting), but the
        // out-arena offsets remain valid.
        let reuse = self.snapshots.get(&wid).copied().filter(|old| {
            old.nodes.len == span
                && old.paints.len == paints_len
                && old.shape_links.len == links_len
        });

        let (nodes_start, paints_start, links_start) = if let Some(old) = reuse {
            (old.nodes.start, old.paints.start, old.shape_links.start)
        } else {
            if let Some(old) = self.snapshots.remove(&wid) {
                self.release(old);
            }
            (
                self.rows.items.len() as u32,
                self.paints.items.len() as u32,
                self.shape_links.items.len() as u32,
            )
        };

        let node_spans = &cascades.paint_arena.node_spans;
        let e_wid = entries.widget_id();
        let e_rect = entries.rect();
        let e_sense = entries.sense();
        let e_focus = entries.focusable();
        let e_dis = entries.disabled();
        let e_layout = entries.layout_rect();
        if reuse.is_some() {
            let base = nodes_start as usize;
            self.rows.items[base..base + span as usize].copy_from_slice(&cascades.rows[lo..hi]);
            self.sptrs[base..base + span as usize]
                .copy_from_slice(&cascades.subtree_paint_rects[lo..hi]);
            for offset in 0..span as usize {
                let gi = entries_base as usize + lo + offset;
                self.entry_rows[base + offset] = EntryRow {
                    widget_id: e_wid[gi],
                    rect: e_rect[gi],
                    sense: e_sense[gi],
                    focusable: e_focus[gi],
                    disabled: e_dis[gi],
                    layout_rect: e_layout[gi],
                };
                let s = node_spans[lo + offset];
                let rel_start = if s.len == 0 {
                    0
                } else {
                    s.start - paint_capture_start
                };
                self.paint_spans[base + offset] = Span::new(rel_start, s.len);
            }
        } else {
            self.rows.items.extend_from_slice(&cascades.rows[lo..hi]);
            self.sptrs
                .extend_from_slice(&cascades.subtree_paint_rects[lo..hi]);
            for i in lo..hi {
                let gi = entries_base as usize + i;
                self.entry_rows.push(EntryRow {
                    widget_id: e_wid[gi],
                    rect: e_rect[gi],
                    sense: e_sense[gi],
                    focusable: e_focus[gi],
                    disabled: e_dis[gi],
                    layout_rect: e_layout[gi],
                });
            }
            for &s in &node_spans[lo..hi] {
                let rel_start = if s.len == 0 {
                    0
                } else {
                    s.start - paint_capture_start
                };
                self.paint_spans.push(Span::new(rel_start, s.len));
            }
            self.rows.acquire(span);
        }

        let src_paints =
            &cascades.paint_arena.rows[paint_capture_start as usize..paint_capture_end as usize];
        if reuse.is_some() {
            let base = paints_start as usize;
            self.paints.items[base..base + paints_len as usize].copy_from_slice(src_paints);
        } else {
            self.paints.items.extend_from_slice(src_paints);
            self.paints.acquire(paints_len);
        }

        if reuse.is_some() {
            let base = links_start as usize;
            let mut idx = 0usize;
            for abs_shape in shape_base..shape_end {
                let abs_paint = shape_to_paint[abs_shape as usize];
                if abs_paint == u32::MAX {
                    continue;
                }
                self.shape_links.items[base + idx] = ShapeLink {
                    rel_shape_idx: abs_shape - shape_base,
                    rel_paint_idx: abs_paint - paint_capture_start,
                };
                idx += 1;
            }
        } else {
            for abs_shape in shape_base..shape_end {
                let abs_paint = shape_to_paint[abs_shape as usize];
                if abs_paint == u32::MAX {
                    continue;
                }
                self.shape_links.items.push(ShapeLink {
                    rel_shape_idx: abs_shape - shape_base,
                    rel_paint_idx: abs_paint - paint_capture_start,
                });
            }
            self.shape_links.acquire(links_len);
        }

        self.snapshots.insert(
            wid,
            Snapshot {
                key,
                nodes: Span::new(nodes_start, span),
                paints: Span::new(paints_start, paints_len),
                shape_links: Span::new(links_start, links_len),
                root_paint_rect,
            },
        );
        self.captures += 1;
    }

    fn release(&mut self, snap: Snapshot) {
        self.rows.release(snap.nodes.len);
        self.paints.release(snap.paints.len);
        self.shape_links.release(snap.shape_links.len);
    }

    /// Single-pass churn step: bump the per-wid streak, decide whether
    /// to enter/exit cooldown, and report what `capture` should do
    /// next. Touches `self.churn` exactly once via `entry()`. Pure
    /// w.r.t. `self.snapshots` — the caller does any required eviction
    /// based on the returned variant.
    fn update_churn(&mut self, wid: WidgetId, key: ProbeKey) -> ChurnDecision {
        let frame = self.frame_seq;
        let old_snapshot_key = self.snapshots.get(&wid).map(|s| s.key);
        let churn = self.churn.entry(wid).or_insert(Churn {
            last_key: key,
            miss_streak: 0,
            cool_until: 0,
            last_capture_frame: frame,
        });
        // A hit on this wid means `capture` is *not* called that
        // frame — a gap > 1 between successive capture frames is the
        // only "we hit in between" signal available. Reset streak so
        // a captured snapshot that did amortize doesn't count toward
        // the next cooldown.
        if frame.wrapping_sub(churn.last_capture_frame) > 1 {
            churn.miss_streak = 0;
        }
        churn.last_capture_frame = frame;

        // Cooldown still in effect? `cool_until > frame` under
        // wrapping `u32` arithmetic — use the half-range trick so the
        // comparison survives the `wrapping_add` in the entry path.
        let cooling =
            churn.cool_until != frame && churn.cool_until.wrapping_sub(frame) < u32::MAX / 2;
        if cooling {
            if churn.last_key == key {
                // Two consecutive identical keys during cooldown →
                // thrash settled. Exit cooldown and capture; any
                // prior snapshot was evicted on entry, so no extra
                // evict needed.
                churn.cool_until = 0;
                churn.miss_streak = 0;
                return ChurnDecision::Capture;
            }
            churn.last_key = key;
            return ChurnDecision::Skip;
        }

        // Not cooling. If a prior snapshot exists with a different
        // key, this is a wasted capture — bump the streak.
        // `old.key == key` is unreachable (probe would have hit
        // first and `capture` wouldn't be called).
        if let Some(old_key) = old_snapshot_key {
            debug_assert!(old_key != key, "capture called when probe would have hit");
            churn.miss_streak = churn.miss_streak.saturating_add(1);
            if churn.miss_streak >= CHURN_THRESHOLD {
                churn.cool_until = frame.wrapping_add(COOL_FRAMES);
                churn.last_key = key;
                return ChurnDecision::SkipAndEvict;
            }
        }
        churn.last_key = key;
        ChurnDecision::Capture
    }

    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.release(snap);
            }
            self.churn.remove(wid);
        }
    }
}
