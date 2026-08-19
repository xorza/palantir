//! Per-frame layout scratch: everything the measure and arrange passes
//! build up and throw away, with its capacity kept across frames.

use crate::layout::LayerLayout;
use crate::layout::cache::{AvailableKey, CachedSubtree, INVALID_AVAILABLE};
use crate::layout::counters::LayoutCounters;
use crate::layout::grid::grid_context::GridContext;
use crate::layout::intrinsic::SLOT_COUNT;
use crate::layout::stack::StackScratch;
use crate::layout::wrapstack::WrapScratch;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::scene::tree::Tree;

/// `LayoutScratch::arrange_src` entry for a node whose subtree measure did
/// not restore from the cache — arrange must run the drivers for it.
/// `u32::MAX` is unreachable as an arena index: the snapshot holds one row
/// per node and a tree that large exhausts memory first.
pub(super) const NO_ARRANGE_SRC: u32 = u32::MAX;

/// Per-frame intermediate state: every field is reset / overwritten at
/// the top of [`LayoutEngine::run`] and exists only for the duration of
/// the layout pass. Capacity is retained across frames so steady state
/// is alloc-free.
///
/// - `grid` — grid-driver scratch (per-depth track state, hug pool).
/// - `wrap` — wrapstack flat per-depth line buffer.
/// - `desired` — measure-pass output, read by arrange.
/// - `intrinsics` — intra-frame cache for `intrinsic(node, axis, req)`
///   queries. Pure function of subtree; safe to
///   memoize within a frame. Flat `Vec` indexed by node, four slots
///   per node (one per `(axis, req)` combination). NaN means "not yet
///   computed".
/// - `available_q` — quantized offer per node, the key
///   [`MeasureCache`] records a subtree under.
/// - `arrange_src` — snapshot arena base of each subtree measure
///   restored from the cache this frame.
/// - `stack_fill` — Fill-freeze scratch, same depth-shared shape as
///   `wrap`.
/// - `counters` — test-only observability for the run.
/// ## Cache-hit contract
///
/// Fields split into three lifecycle categories:
///
/// 1. **Drained on measure exit** — `wrap.pool`, `stack_fill.pool`,
///    `grid.depth_stack`, `grid.track_aggregator`. Driver stacks:
///    pushed on enter, truncated on exit, so a [`MeasureCache`] hit that
///    skips a subtree's measure is invisible to them — they were never
///    going to carry state out. (`stack_fill.pool` and
///    `grid.depth_stack` are used by arrange too, but rebuild their own
///    state rather than reading measure's.)
///
/// 2. **Retained measure → arrange/record** — `desired`,
///    `LayerLayout::scroll_content`, and `grid.track_state`.
///    `desired` is node-indexed and the cache transparently round-
///    trips it through [`CachedSubtree::desired`]. Scroll content is
///    likewise node-indexed and restored into the current layout
///    result for the next record pass. `grid.track_state` is
///    indexed per-grid (not per-node) so the cache hit path has to
///    explicitly call [`restore_after_cache_hit`] to splat
///    [`CachedSubtree::tracks`] back into the live pool — without
///    that, arrange reads zeros and every cell collapses to (0, 0).
///
/// 3. **Node-indexed measure memos, round-tripped by the cache** —
///    `intrinsics` and `available_q`. Not stacks: `resize_for` fills
///    them per node and nothing truncates them. They look drainable
///    because arrange never queries them, but they *do* carry state out
///    — [`MeasureCache::capture_tree`] reads both after arrange, so on a
///    cache-hit subtree (whose slots measure never filled) they have to
///    be splatted back by [`restore_after_cache_hit`] first or the next
///    snapshot records NaN for `intrinsics` and `INVALID_AVAILABLE` for
///    `available_q` — which silently makes that subtree uncacheable from
///    then on.
///
/// **Adding a new field to category (2)** takes three coordinated
/// edits: a column in the whole-tree snapshot, a [`CachedSubtree`]
/// field carrying it through the cache, and a restore branch inside
/// [`restore_after_cache_hit`]. All four sites are compiler-enforced —
/// `capture_tree` and `restore_after_cache_hit` destructure
/// exhaustively, the other two are struct literals — so a missed edit
/// is a build error, not a silent arrange corruption. The reset
/// functions (`NodeArenas::clear`, `LayerLayout::resize_for`, and the
/// one below) destructure to buy the same thing for a field left
/// un-reset. Behaviour is pinned per-driver by the fixtures in
/// `src/layout/cache/integration_tests.rs`.
///
/// `arrange_src` is the one category-(2) field arrange *consumes* rather
/// than reads through: measure stamps the snapshot arena base of every
/// subtree it short-circuited, and
/// [`LayoutPass::replay_arranged`](crate::layout::pass::LayoutPass::replay_arranged)
/// replays that subtree's rects instead of re-running the drivers.
#[derive(Debug, Default)]
pub(crate) struct LayoutScratch {
    /// Test-only observability for this run — see [`LayoutCounters`].
    pub(crate) counters: LayoutCounters,
    pub(super) grid: GridContext,
    pub(super) wrap: WrapScratch,
    pub(super) stack_fill: StackScratch,
    pub(super) desired: Vec<Size>,
    /// Snapshot arena base of each subtree root measure restored from the
    /// cache this frame, or [`NO_ARRANGE_SRC`]. Written at the measure-hit
    /// site, read once per node by `arrange`.
    pub(super) arrange_src: Vec<u32>,
    pub(super) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(super) available_q: Vec<AvailableKey>,
    /// Whether this frame is rebuilding the measure snapshot rather than
    /// reusing the previous one. Decided once at the top of
    /// [`LayoutEngine::run`](crate::layout::engine::LayoutEngine::run) and
    /// read by the capture and cache-restore paths — per-frame state, so
    /// it lives here with the rest of the frame's scratch rather than on
    /// the persistent engine.
    pub(super) cache_rebuild: bool,
}

impl LayoutScratch {
    /// Destructured so a field added to `LayoutScratch` cannot be left
    /// un-reset here. The three driver stacks are reset by their own
    /// drivers on enter/exit, so they are bound and ignored by name
    /// rather than by `..` — that is still a decision the compiler makes
    /// someone make.
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        let Self {
            counters: _,
            // Decided by `LayoutEngine::run` before the layer loop this
            // runs inside, so resetting it here would wipe the answer.
            cache_rebuild: _,
            grid,
            wrap: _,
            stack_fill: _,
            desired,
            arrange_src,
            intrinsics,
            available_q,
        } = self;
        desired.clear();
        desired.resize(n, Size::ZERO);
        arrange_src.clear();
        arrange_src.resize(n, NO_ARRANGE_SRC);
        intrinsics.clear();
        intrinsics.resize(n, [f32::NAN; SLOT_COUNT]);
        available_q.clear();
        available_q.resize(n, INVALID_AVAILABLE);
        grid.track_state.reset_for(tree);
    }

    /// Splat every per-subtree side-state column carried by `arenas` back
    /// into the live pools after a measure-cache hit. Owns the dispatch
    /// over every retained category-(2) field: scroll content, text shapes
    /// (appended to the live frame buffer with per-node spans rebased), and
    /// per-grid hug arrays. Adding a new retained driver column adds one branch
    /// here so the engine's cache-hit path stays a single call. On
    /// `LayoutScratch` rather than on `LayoutEngine` because the caller holds
    /// an immutable borrow of `engine.cache` via the cached-subtree handle —
    /// taking `&mut self` here and `&mut LayerLayout` separately keeps those
    /// borrows disjoint. Pinned by
    /// `cache::integration_tests::cache_hit_preserves_grid_cell_rects`
    /// and the per-driver `cache_hit_preserves_*_rects` fixtures.
    /// `#[inline]`-marked because every cache hit takes this path and the
    /// grid-free common path is a single bitset test.
    #[inline]
    pub(super) fn restore_after_cache_hit(
        &mut self,
        tree: &Tree,
        subtree: std::ops::Range<usize>,
        cached: &CachedSubtree<'_>,
        layer: &mut LayerLayout,
    ) {
        // Destructured exhaustively — no `..` — so a new `CachedSubtree`
        // column cannot be captured and then silently never restored. That
        // is the failure `LayoutScratch`'s doc warns about in prose ("three
        // coordinated edits … forgetting any one corrupts arrange
        // silently"); `capture_tree` already destructures its input the
        // same way, so this closes the other end.
        //
        // The three `_` bindings are the fields that are deliberately not
        // this function's job: `root` and `nodes_base` describe the
        // snapshot rather than being columns of it, and `desired` is
        // restored by the measure-hit site itself
        // (`LayoutPass::replay_arranged`'s caller) because it is what
        // decides the hit.
        let CachedSubtree {
            root: _,
            nodes_base: _,
            desired: _,
            scroll_content,
            text_spans,
            intrinsics,
            available_q,
            tracks,
            text_shapes,
            text_shapes_base,
        } = cached;

        layer.scroll_content[subtree.clone()].copy_from_slice(scroll_content);
        // Append the snapshot's flat text-shape range to the live
        // per-frame buffer, then rebase its subtree-local spans by
        // `dest_start` into the per-node `text_spans` column.
        let dest_start = layer.text_shapes.len() as u32;
        layer.text_shapes.extend_from_slice(text_shapes);
        for (i, snap_span) in text_spans.iter().copied().enumerate() {
            layer.text_spans[subtree.start + i] = if snap_span.len == 0 {
                Span::default()
            } else {
                Span {
                    start: dest_start + snap_span.start - *text_shapes_base,
                    len: snap_span.len,
                }
            };
        }
        if self.cache_rebuild {
            for (dst, src) in self.intrinsics[subtree.clone()].iter_mut().zip(*intrinsics) {
                for (dst_slot, src_slot) in dst.iter_mut().zip(src) {
                    if dst_slot.is_nan() {
                        *dst_slot = *src_slot;
                    }
                }
            }
            self.available_q[subtree.clone()].copy_from_slice(available_q);
        }
        // `grid.track_state` — gated on `Tree::subtree_has_grid` (one bit-test
        // off the same `subtree_end` word the caller already read) so
        // grid-free subtrees pay nothing.
        if tree.subtree_has_grid(subtree.start) {
            self.grid.track_state.restore_subtree(tree, subtree, tracks);
        }
    }
}
