use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::element::{LayoutCore, LayoutMode};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::axis::Axis;
use crate::layout::cache::{MeasureCache, SubtreeArenas, quantize_available};
use crate::layout::grid::GridContext;
use crate::layout::intrinsic::{LenReq, SLOT_COUNT};
use crate::layout::scroll::ScrollStates;
use crate::layout::stack::StackScratch;
use crate::layout::support::{
    AxisCtx, LeafTextShape, TextCtx, leaf_text_shapes, resolve_axis_size, stretched_extent,
    zero_subtree,
};
use crate::layout::types::align::HAlign;
use crate::layout::types::sizing::Sizing;
use crate::layout::wrapstack::WrapScratch;
use crate::layout::{
    LayerLayout, Layout, ShapedText, canvas, grid, intrinsic, scroll, stack, wrapstack, zstack,
};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Sums;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::shape::TextWrap;
use crate::text::{LineFit, ShapeParams, TextShaper};
use rustc_hash::FxHashSet;

/// Per-frame intermediate state: every field is reset / overwritten at
/// the top of [`LayoutEngine::run`] and exists only for the duration of
/// the layout pass. Capacity is retained across frames so steady state
/// is alloc-free.
///
/// - `grid` — grid-driver scratch (per-depth track state, hug pool).
/// - `wrap` — wrapstack flat per-depth line buffer.
/// - `desired` — measure-pass output, read by arrange.
/// - `intrinsics` — intra-frame cache for `intrinsic(node, axis, req)`
///   queries (see `intrinsic.md`). Pure function of subtree; safe to
///   memoize within a frame. Flat `Vec` indexed by node, four slots
///   per node (one per `(axis, req)` combination). NaN means "not yet
///   computed".
/// - `tmp_hugs` — staging buffer for the next [`MeasureCache`]
///   snapshot's per-grid hug payload. Filled by
///   `GridHugStore::snapshot_subtree` then handed to
///   `MeasureCache::write_subtree`.
///
/// Module-internal tests (e.g. `stack/tests.rs`) reach in via
/// `pub(crate)` to pin measure output independently of
/// arrange's slot-clamping.
///
/// ## Cache-hit contract
///
/// Fields split into two lifecycle categories:
///
/// 1. **Drained on measure exit** — `wrap.pool`,
///    `stack_fill.pool`, `grid.depth_stack`, `grid.track_aggregator`,
///    `intrinsics`, `tmp_hugs`. Each driver pushes on enter and
///    truncates on exit; arrange never reads them. A
///    [`MeasureCache`] hit that skips a subtree's measure is
///    invisible to these — they were never going to carry state out.
///
/// 2. **Retained measure → arrange** — `desired` and `grid.hugs`.
///    `desired` is node-indexed and the cache transparently round-
///    trips it through [`SubtreeArenas::desired`]. `grid.hugs` is
///    indexed per-grid (not per-node) so the cache hit path has to
///    explicitly call [`Self::restore_after_cache_hit`] to splat
///    [`SubtreeArenas::hugs`] back into the live pool — without
///    that, arrange reads zeros and every cell collapses to (0, 0).
///
/// **Adding a new field to category (2)** requires three coordinated
/// edits: a snapshot writer into [`Self::tmp_hugs`]-style staging
/// (or a new arena), a [`SubtreeArenas`] field carrying it through
/// the cache, and a restore branch inside the free function
/// [`restore_after_cache_hit`] in this module. Forgetting any one
/// corrupts arrange silently — pinned per-driver by the fixtures in
/// `src/layout/cache/integration_tests.rs`.
#[derive(Default)]
pub(crate) struct LayoutScratch {
    pub(crate) grid: GridContext,
    pub(crate) wrap: WrapScratch,
    pub(crate) stack_fill: StackScratch,
    pub(crate) desired: Vec<Size>,
    pub(crate) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(crate) tmp_hugs: Vec<f32>,
    /// Count of `intrinsic::compute` (cache-miss) calls this frame —
    /// test observability for the intrinsic cache. Reset at the top of
    /// `run`; read by tests to assert a localized change doesn't trigger
    /// a whole-tree intrinsic re-walk. Test-only so production / bench
    /// builds carry no counter in the hot `intrinsic` path.
    #[cfg(test)]
    pub(crate) intrinsic_computes: u32,
    /// Subtree roots restored from the measure cache this `run` —
    /// test observability so cache-hit tests can assert the warm
    /// frame actually hit (and where) instead of passing vacuously
    /// when hash stability regresses and every lookup misses.
    #[cfg(test)]
    pub(crate) cache_hits: Vec<WidgetId>,
}

impl LayoutScratch {
    fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.intrinsics.clear();
        self.intrinsics.resize(n, [f32::NAN; SLOT_COUNT]);
        self.grid.hugs.reset_for(tree);
    }
}

/// Persistent layout engine. Field groups by lifetime:
///
/// - `scratch` — per-frame intermediate state (see [`LayoutScratch`]).
///   Cleared at the top of every `run`.
/// - `active_layer` — which layer's slot the recursive measure/arrange
///   currently writes to. Set at the top of each iteration in `run`;
///   between/outside `run` invocations its value is whatever the last
///   iteration left, but no recursive code runs there to read it.
/// - `scroll_states` — cross-frame `WidgetId → ScrollLayoutState` for
///   every scroll widget (see the field doc below).
/// - `cache` — cross-frame measure cache. See [`cache`] and
///   `src/layout/measure-cache.md`.
///
/// Per-frame *output* is **not** held here: `run` threads it through an
/// `out: &mut Layout` (one [`LayerLayout`] slot per `Layer`, written via
/// `out[self.active_layer]`), so the finalized layout is owned by the
/// caller and read by the encoder, cascade, hit-index, scroll-state
/// refresh, and tests.
#[derive(Default)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(crate) active_layer: Layer,
    /// Cross-frame `WidgetId → ScrollLayoutState` for every scroll
    /// widget. Owned here (not in `StateMap`) because the contents
    /// are layout-derived; the scroll driver writes layout fields
    /// during measure + arrange, the widget reads at record time and
    /// mutates `offset` from input. Keyed by the inner viewport
    /// node's id — see [`scroll::ScrollLayoutState`].
    pub(crate) scroll_states: ScrollStates,
    pub(crate) cache: MeasureCache,
}

/// Quantum (inverse) for wrap-target quantization: bucket width is
/// `1.0 / WRAP_QUANTUM_PX_INV` logical pixels (= 1 px). Deliberately
/// matches the measure cache's `available_q` grid
/// (`cache::quantize_available`): the text a cache hit blits was shaped
/// at this width, so the shaping granularity must not be finer than the
/// key that gates the blit — otherwise a sub-pixel parent jitter inside
/// one `available_q` bucket could serve text shaped for a ≤0.5 px-
/// different width (visible as a wrong wrap point, unlike the invisible
/// sub-pixel error on continuous Fill/Hug sizing). Layout policy — lives
/// here, not in `text/`, so the tradeoff is local to its only consumer.
const WRAP_QUANTUM_PX_INV: f32 = 1.0;

#[inline]
fn quantize_wrap_target(v: f32) -> u32 {
    (v.max(0.0) * WRAP_QUANTUM_PX_INV).round() as u32
}

/// Splat every per-subtree side-state column carried by `arenas` back
/// into the live pools after a measure-cache hit. Owns the dispatch
/// over every retained category-(2) field: text shapes (appended to
/// the live frame buffer with per-node spans rebased) and per-grid
/// hug arrays. Adding a new retained driver column adds one branch
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
fn restore_after_cache_hit(
    scratch: &mut LayoutScratch,
    tree: &Tree,
    subtree: std::ops::Range<usize>,
    arenas: &SubtreeArenas<'_>,
    layer: &mut LayerLayout,
) {
    // Append the snapshot's flat text-shape range to the live
    // per-frame buffer, then rebase its subtree-local spans by
    // `dest_start` into the per-node `text_spans` column.
    let dest_start = layer.text_shapes.len() as u32;
    layer.text_shapes.extend_from_slice(arenas.text_shapes);
    for (i, snap_span) in arenas.text_spans.iter().enumerate() {
        layer.text_spans[subtree.start + i] = Span {
            start: dest_start + snap_span.start,
            len: snap_span.len,
        };
    }
    // `grid.hugs` — gated on `Tree::subtree_has_grid` (one bit-test
    // off the same `subtree_end` word the caller already read) so
    // grid-free subtrees pay nothing.
    if tree.subtree_has_grid(subtree.start) {
        scratch
            .grid
            .hugs
            .restore_subtree(tree, subtree, arenas.hugs);
    }
}

/// Full per-node sizing pipeline: derive `inner_avail` from the parent-
/// supplied `available` + `style` + clamps, hand it to the driver via
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
fn resolve_sizing(
    style: LayoutCore,
    available: Size,
    intrinsic_min: Size,
    min_size: Size,
    max_size: Size,
    dispatch: impl FnOnce(Size) -> Size,
) -> Size {
    let Sums {
        horiz: p_horiz,
        vert: p_vert,
    } = style.padding.sums();
    let Sums {
        horiz: m_horiz,
        vert: m_vert,
    } = style.margin.sums();

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
    let outer_w = match style.size.w() {
        Sizing::Fixed(v) => v,
        _ => (dispatch_avail.w - m_horiz).max(0.0),
    }
    .clamp(min_size.w, max_size.w);
    let outer_h = match style.size.h() {
        Sizing::Fixed(v) => v,
        _ => (dispatch_avail.h - m_vert).max(0.0),
    }
    .clamp(min_size.h, max_size.h);
    let inner_avail = Size::new((outer_w - p_horiz).max(0.0), (outer_h - p_vert).max(0.0));

    let content = dispatch(inner_avail);

    // Fold content into margin-inclusive desired. Margin is added once
    // at the end inside `resolve_axis_size`; this function works in
    // margin-exclusive space (`content_plus_padding = content + p_*`).
    Size::new(
        resolve_axis_size(AxisCtx {
            sizing: style.size.w(),
            content_plus_padding: content.w + p_horiz,
            available: available.w,
            intrinsic_min: intrinsic_min.w,
            margin: m_horiz,
            min: min_size.w,
            max: max_size.w,
        }),
        resolve_axis_size(AxisCtx {
            sizing: style.size.h(),
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
    /// Drop cross-frame measure-cache entries and scroll-state rows for
    /// `WidgetId`s that vanished this frame. Called from `Ui::frame`
    /// with the same `removed` slice that `DamageEngine` and `TextShaper`
    /// consume. One iteration over `removed`; both stores are reached
    /// directly because they're co-located on `LayoutEngine`.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for wid in removed {
            if let Some(snap) = self.cache.snapshots.remove(wid) {
                self.cache.nodes.release(snap.nodes.len);
                self.cache.hugs.release(snap.hugs.len);
                self.cache.text_shapes_arena.release(snap.text_shapes.len);
            }
            self.scroll_states.remove(wid);
        }
        self.cache.maybe_compact();
    }

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` under content-sizing `req`. See `intrinsic.md`.
    ///
    /// Pure function of the subtree at `node`: doesn't depend on the
    /// parent's available width or the arranged rect. Memoized via the
    /// intra-frame cache so repeated queries during the same `run` cost
    /// one array load. Consumed by `grid::measure` (Phase 1 column
    /// resolution) and `stack::measure` (Fill min-content floor).
    pub(crate) fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        tc: &TextCtx<'_>,
    ) -> f32 {
        let slot = req.slot(axis);
        let idx = node.idx();
        let cached = self.scratch.intrinsics[idx][slot];
        if !cached.is_nan() {
            return cached;
        }
        // Cross-frame reuse: an unchanged subtree's intrinsic is valid from
        // last frame's measure-cache snapshot. Intrinsic is
        // `available`-independent, so this hits even on a resize frame
        // where the desired-cache (`try_lookup`) misses on `available_q`.
        // Crucially it fires at the *query* site: a parent computes its
        // `intrinsic_min` (which queries children) before measuring those
        // children, so the children's own cache-hit restore comes too late
        // — only a lookup here stops the ancestor cold-recursing through
        // every unchanged sibling subtree.
        let wid = tree.records.widget_id()[idx];
        let hash = tree.rollups.subtree[idx];
        if let Some(v) = self.cache.lookup_root_intrinsic(wid, hash, slot) {
            self.scratch.intrinsics[idx][slot] = v;
            return v;
        }
        #[cfg(test)]
        {
            self.scratch.intrinsic_computes += 1;
        }
        let v = intrinsic::compute(self, tree, node, axis, req, tc);
        self.scratch.intrinsics[idx][slot] = v;
        v
    }

    /// Run measure + arrange for every root in every layer's tree
    /// against `surface` (the viewport rect). Iterates trees in
    /// `Layer::PAINT_ORDER`; each tree's output lands in
    /// `self.result[layer]` directly. Recursive measure/arrange reads
    /// the active slot via `self.active_layer`.
    #[profiling::function]
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        tc: &TextCtx<'_>,
        surface: Rect,
        out: &mut Layout,
    ) {
        assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        #[cfg(test)]
        {
            self.scratch.intrinsic_computes = 0;
            self.scratch.cache_hits.clear();
        }
        let surface_end = surface.max();
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            self.active_layer = layer;
            out[layer].resize_for(tree);
            if tree.records.is_empty() {
                continue;
            }
            self.scratch.resize_for(tree);
            for slot in &tree.roots {
                let root = slot.first_node;
                // Main: implicit root spans the surface. Side layers:
                // `slot.placement.anchor` is the paint placement. `slot.placement.size`
                // controls the measurement available:
                //   - `None` → "fill from anchor to bottom-right", so
                //     `available = surface - anchor`. The dropdown /
                //     tooltip default: body never overflows past the
                //     viewport regardless of how its own `Sizing`
                //     resolves.
                //   - `Some(s)` → anchor-independent cap clamped to
                //     the surface size. The caller takes responsibility
                //     for placement (typically via the popup's smart
                //     flip-then-clamp). Decouples measurement from
                //     anchor so a popup near the bottom edge can still
                //     measure against its full content height and flip
                //     upward on the next frame.
                // The root's own `Sizing` governs the painted size
                // within that available — see the `stretched_extent`
                // call below.
                let (origin, available) = if layer == Layer::Main {
                    (surface.min, surface.size)
                } else {
                    let available = match slot.placement.size {
                        None => {
                            let rem = (surface_end - slot.placement.anchor).max(glam::Vec2::ZERO);
                            Size::new(rem.x, rem.y)
                        }
                        Some(s) => Size::new(s.w.min(surface.size.w), s.h.min(surface.size.h)),
                    };
                    (slot.placement.anchor, available)
                };
                let desired = self.measure(tree, root, available, tc, out);
                // The layer engine *is* the parent for the root, so it
                // does WPF Stretch's arrange-time grow here. Modeled
                // as a synthetic `Fixed`-sized parent of size
                // `available` so `stretched_extent` returns
                // `available.max(desired)` for Fill roots (stretch to
                // the layer's slot, rigid descendants overflow past
                // it) and `desired` for Hug/Fixed roots. Main's
                // implicit viewport root is `Fill × Fill`, so both
                // axes resolve to `available.max(desired)` — same as
                // the prior surface-fill behavior.
                let root_size = tree.records.layout()[root.idx()].size;
                let synth_parent_w = Sizing::Fixed(available.w);
                let synth_parent_h = Sizing::Fixed(available.h);
                let size = Size::new(
                    stretched_extent(root_size.w(), desired.w, available.w, synth_parent_w),
                    stretched_extent(root_size.h(), desired.h, available.h, synth_parent_h),
                );
                // Root has no parent — pass its own outer size as a
                // sensible default for any driver that reads
                // `parent_outer` (today only scroll, when mounted as
                // root with no wrapper ZStack).
                self.arrange(tree, root, size, Rect { min: origin, size }, out);
            }
        }
        assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores the resolved size for each visited node in
    /// `self.desired` (read by `arrange`).
    pub(crate) fn measure(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available: Size,
        tc: &TextCtx<'_>,
        out: &mut Layout,
    ) -> Size {
        let style = tree.records.layout()[node.idx()];
        if style.visibility().is_collapsed() {
            self.scratch.desired[node.idx()] = Size::ZERO;
            return Size::ZERO;
        }

        // Phase-2 measure-cache short-circuit: any node. Same
        // `WidgetId`, same rolled subtree hash, same quantized
        // `available` → restore the *whole subtree*'s `desired` and
        // text shapes from last frame's snapshot and skip recursion
        // entirely. The subtree-hash rollup guarantees structural and
        // authoring equivalence; `available_q` guards against parent
        // resize since outer-leaf measure is `available`-dependent
        // for `Hug` / `Fill` axes.
        let cache_wid = tree.records.widget_id()[node.idx()];
        let cache_hash = tree.rollups.subtree[node.idx()];
        let cache_avail = quantize_available(available);
        if let Some(hit) = self.cache.try_lookup(cache_wid, cache_hash, cache_avail) {
            #[cfg(test)]
            self.scratch.cache_hits.push(cache_wid);
            let curr_start = node.idx();
            let curr_end = curr_start + hit.arenas.desired.len();
            // Subtree hash includes child count + per-child rollups,
            // so a length mismatch here would mean the rollup is broken.
            assert_eq!(curr_end, tree.subtree_end_of(curr_start) as usize);
            self.scratch.desired[curr_start..curr_end].copy_from_slice(hit.arenas.desired);
            restore_after_cache_hit(
                &mut self.scratch,
                tree,
                curr_start..curr_end,
                &hit.arenas,
                &mut out[self.active_layer],
            );
            return hit.root;
        }

        // Mark where this subtree's text shapes start in the flat
        // per-frame buffer. After dispatch returns, the subtree owns
        // `[text_shapes_lo..text_shapes.len())` contiguously (pre-order
        // append + nested cache-hit appends both fall inside this
        // range). Used to rebase per-node spans to subtree-local form
        // when snapshotting below.
        let text_shapes_lo = out[self.active_layer].text_shapes.len() as u32;

        let bounds = tree.bounds(node);
        let (min_size, max_size) = (bounds.min_size, bounds.max_size);

        // Min-content intrinsic — the smallest this node can shrink
        // to without breaking a rigid descendant (Fixed widget,
        // explicit `min_size`, longest unbreakable word). Fed into
        // `resolve_desired` as the lower bound under flex semantics:
        // Hug/Fill clamp down to `available` but never below
        // `intrinsic_min`. Cached per (node, axis, slot) so repeat
        // queries during the same `run` are O(1).
        //
        // Per-axis gate: `Sizing::Fixed` ignores `intrinsic_min` in
        // both `resolve_axis_size` (Fixed branch returns `v` verbatim)
        // and the `dispatch_avail.max(intrinsic_min)` floor below
        // (Fixed reads neither side). Skip the query on Fixed axes so
        // a Fixed leaf doesn't trigger a subtree intrinsic walk every
        // frame.
        let intrinsic_min = Size::new(
            match style.size.w() {
                Sizing::Fixed(_) => 0.0,
                _ => self.intrinsic(tree, node, Axis::X, LenReq::MinContent, tc),
            },
            match style.size.h() {
                Sizing::Fixed(_) => 0.0,
                _ => self.intrinsic(tree, node, Axis::Y, LenReq::MinContent, tc),
            },
        );

        // Derive `inner_avail`, dispatch to the driver, fold its raw
        // content into a margin-inclusive `desired`. `resolve_sizing`
        // contains the rationale for each step (intrinsic_min floor,
        // outer clamp to `[min, max]`, single-dispatch monotonicity).
        let desired = resolve_sizing(
            style,
            available,
            intrinsic_min,
            min_size,
            max_size,
            |inner_avail| self.measure_dispatch(tree, node, style, inner_avail, tc, out),
        );

        self.scratch.desired[node.idx()] = desired;

        // Snapshot the entire subtree we just (re)measured. Pre-order
        // arena means the subtree is `[node.idx() .. subtree_end[i]]`
        // contiguous in both `desired` and `text_shapes`. Capacity
        // retains across frames via `clear() + extend_from_slice`
        // inside `MeasureCache::write_subtree`. Per-grid hug arrays
        // for descendant Grids land in `scratch.tmp_hugs` first;
        // empty for grid-free subtrees.
        //
        // Skip leaves: on a fully-cached frame the root's try_lookup
        // hits and the whole subtree is restored in one shot —
        // descendant leaves are never visited, so caching them costs
        // a `WidgetIdMap` insert + three arena copy/acquire pairs
        // per leaf per miss frame for work that the lower-level
        // `TextShaper` cache (`shape_unbounded` keyed on
        // `(WidgetId, ordinal, node_hash)`) already covers.
        // Bench: `caches/{measure,heavy/measure,dense/measure}/forced_miss`
        // improves 4.5–6.6%, `cached` arms neutral (the leaf entries
        // they used to populate are never read on the hit path).
        if style.mode != LayoutMode::Leaf {
            let start = node.idx();
            let end = tree.subtree_end_of(start) as usize;
            self.scratch.tmp_hugs.clear();
            if tree.subtree_has_grid(start) {
                self.scratch.grid.hugs.snapshot_subtree(
                    tree,
                    start..end,
                    &mut self.scratch.tmp_hugs,
                );
            }
            // Hand the per-frame `text_spans` slice straight to the
            // cache; `write_subtree` rebases via `text_spans_base` as
            // it copies, eliminating a scratch buffer + memcpy round
            // trip. Empty spans (`Span::default()` with start=0)
            // round-trip through `saturating_sub` correctly: 0 - lo
            // = 0.
            let text_shapes_hi = out[self.active_layer].text_shapes.len() as u32;
            // Snapshot the root's intrinsics (MinContent is always populated
            // by the `intrinsic_min` query above; MaxContent only if it was
            // queried). Served back by `lookup_root_intrinsic` from
            // `intrinsic()` on a later frame — not on the measure-cache hit
            // path, which would be too late: a parent queries a child's
            // intrinsic before it measures that child.
            let root_intrinsics = self.scratch.intrinsics[start];
            self.cache.write_subtree(
                cache_wid,
                cache_hash,
                cache_avail,
                root_intrinsics,
                SubtreeArenas {
                    desired: &self.scratch.desired[start..end],
                    text_spans: &out[self.active_layer].text_spans[start..end],
                    text_spans_base: text_shapes_lo,
                    hugs: &self.scratch.tmp_hugs,
                    text_shapes: &out[self.active_layer].text_shapes
                        [text_shapes_lo as usize..text_shapes_hi as usize],
                },
            );
        }

        desired
    }

    /// Dispatch one driver measure for `node` against the
    /// already-derived `inner_avail`; returns the driver's raw content
    /// size. Called exactly once per `measure` (single dispatch — see
    /// `resolve_sizing` for why no re-measure is needed when a Fill
    /// axis grows past `available`); the caller folds content into a
    /// margin-inclusive `desired` via `resolve_axis_size`.
    ///
    /// ## Driver contract
    ///
    /// Every layout driver (`stack`, `wrapstack`, `zstack`, `canvas`,
    /// `grid`) is a free module exporting three `pub(crate) fn`s,
    /// matched into here and into [`Self::arrange`] / `intrinsic::compute`:
    ///
    /// - `measure(layout, tree, node, [variant_payload,] inner_avail, tc) -> Size`
    ///   — bottom-up. Recurses into children via `layout.measure(...)`.
    ///   Returns the driver's content size (pre-padding/margin/clamp;
    ///   the caller in [`Self::measure`] folds those in).
    /// - `arrange(layout, tree, node, [variant_payload,] inner)`
    ///   — top-down. Assigns each child a final rect and recurses via
    ///   `layout.arrange(...)`.
    /// - `intrinsic(layout, tree, node, [variant_payload,] axis, req, tc) -> f32`
    ///   — pure on-demand query. Used by `grid::measure` Phase-1 column
    ///   resolution and `stack::measure` Fill min-content floor.
    ///
    /// `variant_payload` carries any per-instance config the driver
    /// needs from `LayoutMode`: `Axis::X`/`Axis::Y` for HStack/VStack
    /// and WrapHStack/WrapVStack (a single function pair per pack
    /// orientation), `idx: u16` for `Grid(idx)`. ZStack and Canvas have
    /// no payload.
    ///
    /// Stack and WrapStack `intrinsic` additionally take both a
    /// `main_axis` and `query_axis` because the answer genuinely depends
    /// on both ("size on Y given you pack on X"). ZStack/Canvas/Grid
    /// take only `axis` — they have no main axis to ask about.
    ///
    /// Adding a new driver = (1) new `LayoutMode` variant, (2) new
    /// module exporting the triple, (3) match arms in this dispatcher,
    /// `arrange`, and `intrinsic::content_intrinsic`. The compiler
    /// flags the missing arms because `LayoutMode` matches are
    /// exhaustive.
    fn measure_dispatch(
        &mut self,
        tree: &Tree,
        node: NodeId,
        style: LayoutCore,
        inner_avail: Size,
        tc: &TextCtx<'_>,
        out: &mut Layout,
    ) -> Size {
        match style.mode {
            LayoutMode::Leaf => self.leaf_content_size(tree, node, inner_avail.w, tc, out),
            LayoutMode::HStack => stack::measure(self, tree, node, inner_avail, Axis::X, tc, out),
            LayoutMode::VStack => stack::measure(self, tree, node, inner_avail, Axis::Y, tc, out),
            LayoutMode::WrapHStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::X, tc, out)
            }
            LayoutMode::WrapVStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::Y, tc, out)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, inner_avail, tc, out),
            LayoutMode::Canvas => canvas::measure(self, tree, node, inner_avail, tc, out),
            LayoutMode::Grid => {
                grid::measure(self, tree, node, style.mode_payload, inner_avail, tc, out)
            }
            // Scroll viewport. INF-axis measure of children; the
            // driver also writes the panned-axis content extent into
            // the persistent `ScrollLayoutState` row (see
            // `scroll::measure`). Pan mask carried in `mode_payload`.
            LayoutMode::Scroll => {
                scroll::measure(self, tree, node, inner_avail, style.mode_payload, tc, out)
            }
        }
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in the
    /// active layer's `Layout`.
    pub(crate) fn arrange(
        &mut self,
        tree: &Tree,
        node: NodeId,
        parent_outer: Size,
        slot: Rect,
        out: &mut Layout,
    ) {
        let style = tree.records.layout()[node.idx()];
        if style.visibility().is_collapsed() {
            zero_subtree(self, tree, node, slot.min, out);
            return;
        }
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        out[self.active_layer].rect[node.idx()] = rendered;
        let inner = rendered.deflated_by(style.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, Axis::X, out),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, Axis::Y, out),
            LayoutMode::WrapHStack => wrapstack::arrange(self, tree, node, inner, Axis::X, out),
            LayoutMode::WrapVStack => wrapstack::arrange(self, tree, node, inner, Axis::Y, out),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner, out),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner, out),
            LayoutMode::Grid => grid::arrange(self, tree, node, inner, style.mode_payload, out),
            LayoutMode::Scroll => scroll::arrange(
                self,
                tree,
                node,
                parent_outer,
                inner,
                style.mode_payload,
                out,
            ),
        }
    }

    /// Walk a Leaf's recorded shapes and return the content size that drives
    /// its hugging. For `ShapeRecord::Text` runs, this is also where shaping
    /// happens: the shaped buffer + measured size land on
    /// `Layout.text_shapes` so the encoder can pick them up later.
    /// `available_w` flows down from the parent and gates wrapping.
    fn leaf_content_size(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        tc: &TextCtx<'_>,
        out: &mut Layout,
    ) -> Size {
        let span_start = out[self.active_layer].text_shapes.len() as u32;
        let mut s = Size::ZERO;
        let mut ordinal: u16 = 0;
        for ts in leaf_text_shapes(tree, tc, node) {
            let m = self.shape_text(tree, node, ordinal, &ts, available_w, tc.shaper, out);
            s = s.max(m);
            ordinal = ordinal.checked_add(1).expect(
                "more than 65535 ShapeRecord::Text per leaf — well past anything sane; \
                 widen the within-node ordinal width if this trips",
            );
        }
        let span_len = out[self.active_layer].text_shapes.len() as u32 - span_start;
        out[self.active_layer].text_spans[node.idx()] = Span {
            start: span_start,
            len: span_len,
        };
        s
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_text(
        &mut self,
        tree: &Tree,
        node: NodeId,
        ordinal: u16,
        ts: &LeafTextShape<'_>,
        available_w: f32,
        text: &TextShaper,
        out: &mut Layout,
    ) -> Size {
        let wid = tree.records.widget_id()[node.idx()];
        let curr_hash = tree.rollups.node[node.idx()];

        // Refresh the unbounded measurement only when the authoring hash
        // has shifted. Crucially, when only the wrap target changed
        // (e.g. animated parent width), the unbounded cache is
        // preserved and only the wrap reshape runs in shape_wrap.
        let unbounded = text.shape_unbounded(
            wid,
            ordinal,
            curr_hash,
            ts.text,
            ShapeParams {
                font_size_px: ts.font_size_px,
                line_height_px: ts.line_height_px,
                max_width_px: None,
                family: ts.family,
                weight: ts.weight,
                halign: HAlign::Auto,
            },
        );

        // Re-shape through the width-bounded path for `Wrap` and the
        // single-line truncating modes against a finite width. For `Wrap`
        // this is needed even when the content fits — the shaped buffer only
        // carries per-line `BufferLine::set_align` when `max_width_px` is
        // `Some`, and a multi-line buffer built without it has every visual
        // line pinned at x = 0; without it an `\n`-separated paragraph that
        // never wraps would render left-aligned while the widget's
        // `cursor_xy` (always called with the wrap target) reads
        // per-line-aligned coords from a different cached buffer. For
        // `SingleLine`/`Ellipsis` it's the path that cuts the run to one
        // line at the committed width.
        let fit = match ts.wrap {
            TextWrap::Wrap | TextWrap::WrapWithOverflow => LineFit::Wrap,
            TextWrap::Ellipsis => LineFit::Ellipsis,
            // `SingleLine`/`Scroll` never reach the bounded branch (excluded
            // below); `Clip` is harmless as their fallthrough value.
            TextWrap::Truncate | TextWrap::SingleLine | TextWrap::Scroll => LineFit::Clip,
        };
        let single_line = matches!(ts.wrap, TextWrap::Truncate | TextWrap::Ellipsis);
        let bounded = matches!(
            ts.wrap,
            TextWrap::Wrap | TextWrap::WrapWithOverflow | TextWrap::Truncate | TextWrap::Ellipsis
        ) && available_w.is_finite();

        let result = if bounded {
            // `WrapWithOverflow` floors the target at the longest word so
            // cosmic never breaks mid-word; `Wrap` lets cosmic glyph-break
            // when a word doesn't fit, so it takes the committed width
            // verbatim. Single-line modes truncate freely.
            let target = if single_line || matches!(ts.wrap, TextWrap::Wrap) {
                available_w
            } else {
                available_w.max(unbounded.intrinsic_min)
            };
            let target_q = quantize_wrap_target(target);
            // Shape at the quantized width, not raw `target`: the measure
            // cache keys on the same 1px grid, so this keeps a cache hit
            // from blitting text shaped for a sub-pixel-different target.
            let target = target_q as f32;
            text.shape_wrap(
                wid,
                ordinal,
                curr_hash,
                ts.text,
                ShapeParams {
                    font_size_px: ts.font_size_px,
                    line_height_px: ts.line_height_px,
                    max_width_px: Some(target),
                    family: ts.family,
                    weight: ts.weight,
                    halign: ts.halign,
                },
                target_q,
                fit,
            )
        } else {
            unbounded
        };

        out[self.active_layer].text_shapes.push(ShapedText {
            measured: result.size,
            key: result.key,
        });
        // A `Scroll` run (single-line editable field) clips + scrolls its own
        // overflow, so its text is scroll content, not layout content: it
        // imposes no width demand on the box. Report zero content width (the
        // height still floors the row) while the shaped buffer above keeps its
        // true measured size for the encoder. Without this the box's `desired`
        // width equals the buffer's natural width, and the WPF Stretch arrange
        // floor (`stack::arrange` freezes each Fill child at its desired size)
        // pins a Fill/Fixed field to its text and refuses to shrink. A Hug
        // field's width comes from its own `min_size.w` reservation instead.
        match ts.wrap {
            TextWrap::Scroll => Size::new(0.0, result.size.h),
            _ => result.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::layoutengine::*;

    /// The wrap target must quantize to the same 1px grid the measure
    /// cache keys on (`cache::quantize_available`), so a sub-pixel parent
    /// jitter inside one `available_q` bucket reshapes text to the
    /// identical width — a cache hit then can't blit text shaped for a
    /// sub-pixel-different target. Trips if either grid changes alone.
    #[test]
    fn wrap_target_matches_cache_grid() {
        // Sub-pixel jitter inside one 1px bucket → identical wrap target.
        assert_eq!(quantize_wrap_target(100.1), quantize_wrap_target(100.4));
        assert_eq!(quantize_wrap_target(99.6), quantize_wrap_target(100.4));
        // Crossing a 1px boundary → different target.
        assert_ne!(quantize_wrap_target(100.4), quantize_wrap_target(100.6));
        // The wrap grid equals the cache's `available_q` rounding.
        for w in [0.0_f32, 99.6, 100.1, 100.4, 250.4] {
            let cache_w = quantize_available(Size::new(w, 0.0)).x;
            assert_eq!(quantize_wrap_target(w) as i32, cache_w, "w={w}");
        }
    }
}
