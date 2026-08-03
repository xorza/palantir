use crate::layout::axis::Axis;
use crate::layout::cache::{
    AvailableKey, CachedSubtree, CaptureTreeInput, INVALID_AVAILABLE, MeasureCache,
    MeasureSnapshot, RootSnapshotKey, quantize_available,
};
use crate::layout::grid::GridContext;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq, SLOT_COUNT};
use crate::layout::pass::LayoutPass;
use crate::layout::probe::{LayoutProbe, PhaseSpan};
use crate::layout::stack::StackScratch;
use crate::layout::support::{AxisCtx, arrange_size, container_text_shapes, resolve_axis_size};
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::wrapstack::WrapScratch;
use crate::layout::{LayerLayout, Layout, intrinsic};
use crate::primitives::interned_str::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Sums;
use crate::primitives::span::Span;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::node::columns::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::record::NodeId;
use crate::scene::tree::recording::RootSlot;
use crate::text::shaper::TextShaper;
use crate::text::system::TextSystem;

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
/// ## Cache-hit contract
///
/// Fields split into two lifecycle categories:
///
/// 1. **Drained on measure exit** — `wrap.pool`,
///    `stack_fill.pool`, `grid.depth_stack`, `grid.track_aggregator`,
///    `intrinsics`. Each driver pushes on enter and
///    truncates on exit; arrange never reads them. A
///    [`MeasureCache`] hit that skips a subtree's measure is
///    invisible to these — they were never going to carry state out.
///
/// 2. **Retained measure → arrange/record** — `desired`,
///    `LayerLayout::scroll_content`, and `grid.hugs`.
///    `desired` is node-indexed and the cache transparently round-
///    trips it through [`CachedSubtree::desired`]. Scroll content is
///    likewise node-indexed and restored into the current layout
///    result for the next record pass. `grid.hugs` is
///    indexed per-grid (not per-node) so the cache hit path has to
///    explicitly call [`restore_after_cache_hit`] to splat
///    [`CachedSubtree::hugs`] back into the live pool — without
///    that, arrange reads zeros and every cell collapses to (0, 0).
///
/// **Adding a new field to category (2)** requires three coordinated
/// edits: a column in the whole-tree snapshot, a [`CachedSubtree`]
/// field carrying it through
/// the cache, and a restore branch inside the free function
/// [`restore_after_cache_hit`] in this module. Forgetting any one
/// corrupts arrange silently — pinned per-driver by the fixtures in
/// `src/layout/cache/integration_tests.rs`.
///
/// `arrange_src` is the one category-(2) field arrange *consumes* rather
/// than reads through: measure stamps the snapshot arena base of every
/// subtree it short-circuited, and [`LayoutEngine::replay_arranged`]
/// replays that subtree's rects instead of re-running the drivers.
#[derive(Debug, Default)]
pub(crate) struct LayoutScratch {
    /// Test-only observability for this run — see [`LayoutProbe`].
    pub(crate) probe: LayoutProbe,
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
}

impl LayoutScratch {
    fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.arrange_src.clear();
        self.arrange_src.resize(n, NO_ARRANGE_SRC);
        self.intrinsics.clear();
        self.intrinsics.resize(n, [f32::NAN; SLOT_COUNT]);
        self.available_q.clear();
        self.available_q.resize(n, INVALID_AVAILABLE);
        self.grid.hugs.reset_for(tree);
    }
}

/// Size offered to one layer root.
///
/// `Layer::Main` fills the surface; every overlay layer derives its own from
/// its [`Placement`](crate::scene::tree::recording::Placement). Shared by
/// [`LayoutEngine::run`], which measures against
/// it, and [`LayoutEngine::cache_snapshot_matches_forest`], which quantizes it
/// into the snapshot's root key — those two **must** agree, or the key
/// describes an offer measure never saw and every root misses.
fn root_available(layer: Layer, slot: &RootSlot, surface: Rect) -> Size {
    if layer == Layer::Main {
        surface.size
    } else {
        slot.placement.available(surface)
    }
}

/// `LayoutScratch::arrange_src` entry for a node whose subtree measure did
/// not restore from the cache — arrange must run the drivers for it.
/// `u32::MAX` is unreachable as an arena index: the snapshot holds one row
/// per node and a tree that large exhausts memory first.
pub(super) const NO_ARRANGE_SRC: u32 = u32::MAX;

/// Persistent layout engine. Field groups by lifetime:
///
/// - `scratch` — per-frame intermediate state (see [`LayoutScratch`]).
///   Cleared at the top of every `run`.
/// - `text` — per-window text shaping and reuse slots.
/// - `cache` — cross-frame measure cache. See [`crate::layout::cache`].
///
/// Per-frame *output* is **not** held here: `run` threads it through an
/// `out: &mut Layout`, so the finalized layout is owned by the caller
/// and read by the encoder, cascade, hit-index, scroll-state refresh,
/// and tests. Recursive work receives only the current [`LayerLayout`]
/// slot.
#[derive(Debug)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(super) cache_rebuild: bool,
    pub(crate) text: TextSystem,
    pub(crate) cache: MeasureCache,
}

/// Splat every per-subtree side-state column carried by `arenas` back
/// into the live pools after a measure-cache hit. Owns the dispatch
/// over every retained category-(2) field: scroll content, text shapes
/// (appended to the live frame buffer with per-node spans rebased), and
/// per-grid hug arrays. Adding a new retained driver column adds one branch
/// here so the engine's cache-hit path stays a single call. Free fn
/// (not a method on `LayoutEngine`) because the caller holds an
/// immutable borrow of `self.cache` via the cached-subtree handle —
/// passing disjoint `&mut LayoutScratch` and `&mut LayerLayout` keeps
/// the borrow checker happy. Pinned by
/// `cache::integration_tests::cache_hit_preserves_grid_cell_rects`
/// and the per-driver `cache_hit_preserves_*_rects` fixtures.
/// `#[inline]`-marked because every cache hit takes this path and the
/// grid-free common path is a single bitset test.
#[inline]
pub(super) fn restore_after_cache_hit(
    scratch: &mut LayoutScratch,
    tree: &Tree,
    subtree: std::ops::Range<usize>,
    cached: &CachedSubtree<'_>,
    layer: &mut LayerLayout,
    cache_rebuild: bool,
) {
    layer.scroll_content[subtree.clone()].copy_from_slice(cached.scroll_content);
    // Append the snapshot's flat text-shape range to the live
    // per-frame buffer, then rebase its subtree-local spans by
    // `dest_start` into the per-node `text_spans` column.
    let dest_start = layer.text_shapes.len() as u32;
    layer.text_shapes.extend_from_slice(cached.text_shapes);
    for (i, snap_span) in cached.text_spans.iter().copied().enumerate() {
        layer.text_spans[subtree.start + i] = if snap_span.len == 0 {
            Span::default()
        } else {
            Span {
                start: dest_start + snap_span.start - cached.text_shapes_base,
                len: snap_span.len,
            }
        };
    }
    if cache_rebuild {
        for (dst, src) in scratch.intrinsics[subtree.clone()]
            .iter_mut()
            .zip(cached.intrinsics)
        {
            for (dst_slot, src_slot) in dst.iter_mut().zip(src) {
                if dst_slot.is_nan() {
                    *dst_slot = *src_slot;
                }
            }
        }
        scratch.available_q[subtree.clone()].copy_from_slice(cached.available_q);
    }
    // `grid.hugs` — gated on `Tree::subtree_has_grid` (one bit-test
    // off the same `subtree_end` word the caller already read) so
    // grid-free subtrees pay nothing.
    if tree.subtree_has_grid(subtree.start) {
        scratch
            .grid
            .hugs
            .restore_subtree(tree, subtree, cached.hugs);
    }
}

/// Full per-node sizing pipeline: derive `inner_avail` from the parent-
/// supplied `available` + `layout` + clamps, hand it to the driver via
/// `dispatch`, fold the driver's raw `content` into a margin-inclusive
/// `desired`. Returns `desired`.
///
/// Per-node padding/margin sums are unpacked once and threaded through
/// both halves of the pipeline (was two unpacks across two free fns
/// before the merge; the grow-detection path that justified the split
/// is gone — single dispatch, no re-measure).
///
/// `available` is the parent-supplied slot (margin-inclusive).
/// `intrinsic_min` floors `available` so children measure against the
/// parent's actual outer size (`max(available, intrinsic_min)` per
/// `resolve_axis_size`) — without this, a Hug grid inside a FILL panel
/// whose own intrinsic_min is pinned by a long sibling would shape
/// children against the smaller surface width. INFINITY-on-Hug-axis
/// preserved (`INF.max(x) == INF`); Fixed axes ignore both inputs in
/// `resolve_axis_size`.
///
/// Single dispatch: when `desired` exceeds `available` on a non-Fixed
/// axis it's because a rigid descendant pinned the floor; a re-dispatch
/// against the grown outer would converge to the same value because
/// every driver's content size is monotone in `available` and pass-1
/// already saturated at the floor. Pinned by
/// `cross_driver_tests::convergence`.
#[inline]
pub(super) fn resolve_sizing(
    layout: LayoutCore,
    available: Size,
    intrinsic_min: Size,
    min_size: Size,
    max_size: Size,
    dispatch: impl FnOnce(Size) -> Size,
) -> Size {
    let Sums {
        horiz: p_horiz,
        vert: p_vert,
    } = layout.padding.sums();
    let Sums {
        horiz: m_horiz,
        vert: m_vert,
    } = layout.margin.sums();

    let dispatch_avail = Size::new(
        available.w.max(intrinsic_min.w),
        available.h.max(intrinsic_min.h),
    );

    // `inner_avail`: outer = `Fixed(v)` on Fixed axes else
    // `dispatch_avail - margin`; clamp outer to `[min_size, max_size]`
    // so a `max_size`-capped parent doesn't grant children more room
    // than it can later arrange; deflate by padding. The clamp matches
    // `resolve_axis_size` below so children's `available` tracks the
    // parent's eventual arranged width.
    let outer_w = layout
        .size
        .w()
        .fixed_value()
        .unwrap_or_else(|| (dispatch_avail.w - m_horiz).max(0.0))
        .clamp(min_size.w, max_size.w);
    let outer_h = layout
        .size
        .h()
        .fixed_value()
        .unwrap_or_else(|| (dispatch_avail.h - m_vert).max(0.0))
        .clamp(min_size.h, max_size.h);
    let inner_avail = Size::new((outer_w - p_horiz).max(0.0), (outer_h - p_vert).max(0.0));

    let content = dispatch(inner_avail);

    // Fold content into margin-inclusive desired. Margin is added once
    // at the end inside `resolve_axis_size`; this function works in
    // margin-exclusive space (`content_plus_padding = content + p_*`).
    Size::new(
        resolve_axis_size(AxisCtx {
            sizing: layout.size.w(),
            content_plus_padding: content.w + p_horiz,
            available: available.w,
            intrinsic_min: intrinsic_min.w,
            margin: m_horiz,
            min: min_size.w,
            max: max_size.w,
        }),
        resolve_axis_size(AxisCtx {
            sizing: layout.size.h(),
            content_plus_padding: content.h + p_vert,
            available: available.h,
            intrinsic_min: intrinsic_min.h,
            margin: m_vert,
            min: min_size.h,
            max: max_size.h,
        }),
    )
}

impl LayoutEngine {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            scratch: LayoutScratch::default(),
            cache_rebuild: false,
            text: TextSystem::new(shaper),
            cache: MeasureCache::default(),
        }
    }

    fn cache_snapshot_matches_forest(
        snapshot: &MeasureSnapshot,
        forest: &Forest,
        surface: Rect,
    ) -> bool {
        if snapshot.nodes.desired.len() != forest.total_nodes()
            || snapshot.roots.len() != forest.total_roots()
        {
            return false;
        }
        let mut root_index = 0;
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            for slot in &tree.roots {
                let root = slot.first_node;
                let available = root_available(layer, slot, surface);
                let current = RootSnapshotKey {
                    wid: tree.records.widget_id()[root.idx()],
                    subtree_hash: tree.rollups.subtree[root.idx()],
                    available_q: quantize_available(available),
                };
                if snapshot.roots[root_index] != current {
                    return false;
                }
                root_index += 1;
            }
        }
        true
    }

    /// Grid's per-track intrinsic aggregator — a bump stack `grid::intrinsic`
    /// extends, recurses through, and truncates back. Reached by name for
    /// the same reason [`LayoutPass`]'s accessors exist; the intrinsic
    /// query itself stays off the pass, so it asks the engine directly.
    #[inline]
    pub(super) fn grid_track_aggregator(&mut self) -> &mut Vec<f32> {
        &mut self.scratch.grid.track_aggregator
    }

    /// Cross-frame intrinsic for one `(node, axis, req)` slot, or `None`
    /// when the node is ineligible or the snapshot has no value.
    ///
    /// Only non-leaf nodes are cacheable: a leaf's intrinsic is cheap to
    /// recompute and it owns no descriptor. Intrinsics are independent of
    /// the parent's `available`, so this checks `subtree_hash` alone and
    /// hits even on a resize frame where `try_lookup` misses on
    /// `available_q`.
    #[inline]
    fn cached_intrinsic(&self, tree: &Tree, idx: usize, slot: usize) -> Option<f32> {
        if LayoutMode::from(tree.records.layout()[idx].meta) == LayoutMode::Leaf {
            return None;
        }
        self.cache.lookup_root_intrinsic(
            tree.records.widget_id()[idx],
            tree.rollups.subtree[idx],
            slot,
        )
    }

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` for the halves `query` asks for.
    ///
    /// Pure function of the subtree at `node`: independent of the
    /// parent's available width and of the arranged rect. Three layers
    /// answer it, cheapest first — the intra-frame slot array, last
    /// frame's snapshot, then a real subtree walk — and only the halves
    /// still missing after the first two reach the walk, so a range query
    /// whose min is already cached costs a max-only recursion.
    ///
    /// Consumed by `grid::measure` (Phase 1 column resolution) and
    /// `stack::measure` (Fill min-content floor) via the thin
    /// [`Self::intrinsic`] / [`Self::intrinsic_range`] wrappers.
    pub(super) fn intrinsic_query(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        let idx = node.idx();
        let mut range = IntrinsicRange::ZERO;
        let (mut missing_min, mut missing_max) = (false, false);
        for (req, slot) in range.requested(query) {
            let cached = self.scratch.intrinsics[idx][req.slot(axis)];
            if !cached.is_nan() {
                *slot = cached;
                continue;
            }
            // Cross-frame reuse: an unchanged subtree's intrinsic is still
            // valid from last frame's measure-cache snapshot. Intrinsic is
            // `available`-independent, so this hits even on a resize frame
            // where the desired-cache (`try_lookup`) misses on
            // `available_q`. Crucially it fires at the *query* site: a
            // parent computes its `intrinsic_min` (which queries children)
            // before measuring those children, so the children's own
            // cache-hit restore comes too late — only a lookup here stops
            // the ancestor cold-recursing through every unchanged sibling
            // subtree.
            if let Some(value) = self.cached_intrinsic(tree, idx, req.slot(axis)) {
                self.scratch.intrinsics[idx][req.slot(axis)] = value;
                *slot = value;
                continue;
            }
            match req {
                LenReq::MinContent => missing_min = true,
                LenReq::MaxContent => missing_max = true,
            }
        }

        let Some(walk) = IntrinsicQuery::of(missing_min, missing_max) else {
            return range;
        };
        self.scratch.probe.intrinsic_computed();
        let computed = intrinsic::compute(self, tree, node, axis, walk, interned_text);
        for (req, slot) in range.requested(walk) {
            let value = computed.get(req);
            self.scratch.intrinsics[idx][req.slot(axis)] = value;
            *slot = value;
        }
        range
    }

    /// One half of [`Self::intrinsic_query`].
    #[inline]
    pub(super) fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        interned_text: &InternedText<'_>,
    ) -> f32 {
        self.intrinsic_query(tree, node, axis, IntrinsicQuery::single(req), interned_text)
            .get(req)
    }

    /// Both halves — what Grid's Hug tracks want, since a track needs the
    /// content range rather than either end of it.
    #[inline]
    pub(super) fn intrinsic_range(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        self.intrinsic_query(tree, node, axis, IntrinsicQuery::range(), interned_text)
    }

    /// Run measure + arrange for every root in every layer's tree
    /// against `surface` (the viewport rect). Iterates trees in
    /// `Layer::PAINT_ORDER`; each tree's recursive work receives a
    /// [`LayoutPass`] bound to that layer's output slot.
    #[profiling::function]
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        interned_text: &InternedText<'_>,
        surface: Rect,
        out: &mut Layout,
    ) {
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        // Once per run, not per layer: `resize_for` runs inside the layer
        // loop and would wipe an earlier layer's counts.
        self.scratch.probe.begin_run();
        self.cache_rebuild =
            !Self::cache_snapshot_matches_forest(&self.cache.previous, forest, surface);
        if self.cache_rebuild {
            self.cache.begin_frame();
        }
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            let layer_out = &mut out[layer];
            layer_out.resize_for(tree);
            if tree.records.is_empty() {
                continue;
            }
            self.scratch.resize_for(tree);
            {
                let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
                for slot in &tree.roots {
                    let root = slot.first_node;
                    let available = root_available(layer, slot, surface);
                    let measure_span = PhaseSpan::start();
                    let desired = pass.measure(root, available);
                    pass.note_measure(measure_span);
                    let root_layout = tree.records.layout()[root.idx()];
                    let size = arrange_size(&root_layout, tree.bounds(root), desired, available);
                    // Overlay policies need the current measured body, not a
                    // response rect retained from an earlier frame.
                    let origin = if layer == Layer::Main {
                        surface.min
                    } else {
                        slot.placement.origin(size, surface)
                    };
                    let arrange_span = PhaseSpan::start();
                    pass.arrange(root, Rect { min: origin, size });
                    pass.note_arrange(arrange_span);
                }
            }
            if self.cache_rebuild {
                self.cache.capture_tree(
                    tree,
                    CaptureTreeInput {
                        desired: &mut self.scratch.desired,
                        rect: &layer_out.rect,
                        scroll_content: &layer_out.scroll_content,
                        intrinsics: &self.scratch.intrinsics,
                        available_q: &mut self.scratch.available_q,
                        grid_hugs: &self.scratch.grid.hugs,
                        text_spans: &layer_out.text_spans,
                        text_shapes: &layer_out.text_shapes,
                    },
                );
            }
            // Container text is paint-only; its wrap width exists only
            // after arrange, so it gets its own pass over the owners the
            // rollup already identified.
            let layouts = tree.records.layout();
            let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
            for index in tree.rollups.container_text.ones() {
                let layout = layouts[index];
                if !layout.meta.visibility().is_visible() {
                    continue;
                }
                let node = NodeId(index as u32);
                let available_w = (pass.rect(node).size.w - layout.padding.horiz()).max(0.0);
                let runs = container_text_shapes(tree, interned_text, node);
                pass.shape_text_runs(node, available_w, runs);
            }
        }
        if self.cache_rebuild {
            self.cache.finish_frame();
        }
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }
}
