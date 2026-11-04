//! Per-frame layout scratch for the measure and arrange passes, with capacity kept across frames.

use crate::common::non_max::NonMaxU32;
use crate::common::span::Span;
use crate::layout::cache::CachedSubtree;
use crate::layout::counters::LayoutCounters;
use crate::layout::drivers::grid::grid_ctx::GridCtx;
use crate::layout::drivers::stack::stack_scratch::StackScratch;
use crate::layout::drivers::wrapstack::WrapScratch;
use crate::layout::measure_columns::MeasureColumns;
use crate::layout::plane_layout::PlaneLayout;
use crate::scene::tree::Tree;
use glam::Vec2;
use std::ops::Range;

/// One inner box a translated replay is inside: where its subtree ends, and its
/// page position.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReplayOrigin {
    pub(super) end: usize,
    pub(super) origin: Vec2,
}

/// Where a measure-cache hit lands: the live tree, the subtree's node range in it, and the plane's output columns.
#[derive(Debug)]
pub(super) struct RestoreTarget<'a> {
    pub(super) tree: &'a Tree,
    pub(super) subtree: Range<usize>,
    pub(super) plane: &'a mut PlaneLayout,
}

/// Per-frame intermediate state, reset at the top of [`LayoutEngine::run`](crate::layout::engine::LayoutEngine::run). Capacity is retained so steady state is alloc-free.
/// ## Cache-hit contract
///
/// Fields fall into three lifecycle categories:
///
/// 1. **Drained on measure exit** — driver stacks, pushed on enter and truncated on exit, so a [`MeasureCache`](crate::layout::cache::MeasureCache) hit that skips a measure is invisible to them.
///
/// 2. **Retained measure → arrange/record** — `measure.desired`, `measure.floor`, `PlaneLayout::scroll_content` and `grid.track_state`. The cache round-trips the node-indexed ones through [`CachedSubtree`]. `grid.track_state` is per-grid, so a hit must call [`Self::restore_after_cache_hit`] to splat [`CachedSubtree::tracks`] back, or every cell collapses to (0, 0).
///
/// 3. **Node-indexed measure memos** — `measure.intrinsics`, `measure.available`, `measure.stable_from`. Nothing truncates them, and [`MeasureCache::capture_tree`](crate::layout::cache::MeasureCache::capture_tree) reads them after arrange, so a hit subtree must splat them back or its snapshot is silently uncacheable.
///
/// **A new measure column** goes on [`MeasureColumns`], which the scratch, the arena and a hit share, and takes a restore branch in [`Self::restore_after_cache_hit`]; exhaustive destructuring enforces both.
///
/// `arrange_src` is frame-local and in none of the three; measure stamps it on each node of a short-circuited subtree, and [`LayoutPass::replay_arranged`](crate::layout::pass::LayoutPass::replay_arranged) replays rects from it. `local` rides beside `PlaneLayout::rect`; `replay_origins` is drained per replay.
#[derive(Debug, Default)]
pub(crate) struct LayoutScratch {
    /// Test-only observability for this run; see [`LayoutCounters`].
    pub(crate) counters: LayoutCounters,
    pub(super) grid: GridCtx,
    pub(super) wrap: WrapScratch,
    pub(super) stack: StackScratch,
    pub(super) measure: MeasureColumns,
    /// Each node's slot origin in its parent's inner box; captured beside `rect` so a translated replay rebuilds it with the same adds.
    pub(super) local: Vec<Vec2>,
    /// Inner-box origins of the nodes a translated replay is inside, innermost last. Drained by every replay.
    pub(super) replay_origins: Vec<ReplayOrigin>,
    /// Snapshot arena row of each node whose subtree measure was restored from the cache this frame. `None` makes arrange run the drivers.
    pub(super) arrange_src: Vec<Option<NonMaxU32>>,
    /// Whether this frame rebuilds the measure snapshot rather than reusing the previous one. Decided at the top of [`LayoutEngine::run`](crate::layout::engine::LayoutEngine::run).
    pub(super) cache_rebuild: bool,
}

impl LayoutScratch {
    /// Destructured so a new field cannot be left un-reset. The driver stacks reset themselves and are ignored by name.
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        let Self {
            counters: _,
            // Decided by `LayoutEngine::run` before the plane loop; resetting here would wipe it.
            cache_rebuild: _,
            grid,
            wrap: _,
            stack: _,
            measure,
            arrange_src,
            local,
            replay_origins: _,
        } = self;
        measure.resize_for(n);
        arrange_src.clear();
        arrange_src.resize(n, None);
        local.clear();
        local.resize(n, Vec2::ZERO);
        grid.track_state.reset_for(tree);
    }

    /// Splat a cache hit's per-subtree columns (desired sizes, scroll content, text shapes, grid hug arrays) back into the live pools, and point each node's arrange at its snapshot row. Here, not on `LayoutEngine`, to keep borrows of `engine.cache` disjoint.
    #[inline]
    pub(super) fn restore_after_cache_hit(
        &mut self,
        cached: &CachedSubtree<'_>,
        target: RestoreTarget<'_>,
    ) {
        let RestoreTarget {
            tree,
            subtree,
            plane,
        } = target;
        // Destructured exhaustively so a new `CachedSubtree` column cannot go unrestored. The `_` binding: `root` describes the snapshot, and the measure-hit site returns it.
        let CachedSubtree {
            root: _,
            rows,
            measure,
            scroll_content,
            text_spans,
            tracks,
            text_shapes,
            text_shapes_base,
        } = cached;
        let MeasureColumns {
            desired,
            floor,
            stable_from,
            intrinsics,
            available,
        } = measure;
        let rows = rows.clone();

        self.measure.desired[subtree.clone()].copy_from_slice(&desired[rows.clone()]);
        // Every node of the subtree, so a descendant arranged at its cached size replays
        // even when the root, at a new size, must dispatch.
        for (src, row) in self.arrange_src[subtree.clone()]
            .iter_mut()
            .zip(rows.clone())
        {
            *src = Some(NonMaxU32::index(row));
        }
        plane.scroll_content[subtree.clone()].copy_from_slice(scroll_content);
        // Append the snapshot's text-shape range to the live buffer, rebasing its spans by `dest_start`.
        let dest_start = plane.text_shapes.len() as u32;
        plane.text_shapes.extend_from_slice(text_shapes);
        for (i, snap_span) in text_spans.iter().copied().enumerate() {
            plane.text_spans[subtree.start + i] = if snap_span.len == 0 {
                Span::default()
            } else {
                Span {
                    start: dest_start + snap_span.start - *text_shapes_base,
                    len: snap_span.len,
                }
            };
        }
        // Arrange reads the floor beside `desired`, so restore it on every hit.
        self.measure.floor[subtree.clone()].copy_from_slice(&floor[rows.clone()]);
        if self.cache_rebuild {
            for (dst, src) in self.measure.intrinsics[subtree.clone()]
                .iter_mut()
                .zip(&intrinsics[rows.clone()])
            {
                for (dst_slot, src_slot) in dst.iter_mut().zip(src) {
                    if dst_slot.is_nan() {
                        *dst_slot = *src_slot;
                    }
                }
            }
            self.measure.available[subtree.clone()].copy_from_slice(&available[rows.clone()]);
            self.measure.stable_from[subtree.clone()].copy_from_slice(&stable_from[rows]);
        }
        // Gated on `Tree::subtree_has_grid` so grid-free subtrees pay nothing.
        if tree.subtree_has_grid(subtree.start) {
            self.grid.track_state.restore_subtree(tree, subtree, tracks);
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::layout::layout_scratch::LayoutScratch;

    impl LayoutScratch {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                counters: _,
                grid,
                wrap,
                stack,
                measure,
                local,
                replay_origins,
                arrange_src,
                cache_rebuild: _,
            } = self;
            grid.scratch_bytes()
                + wrap.scratch_bytes()
                + stack.scratch_bytes()
                + measure.scratch_bytes()
                + scratch_report::vec_bytes(local)
                + scratch_report::vec_bytes(replay_origins)
                + scratch_report::vec_bytes(arrange_src)
        }
    }
}
