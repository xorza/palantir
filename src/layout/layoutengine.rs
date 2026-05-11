use crate::forest::Forest;
use crate::forest::element::{LayoutCore, LayoutMode};
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::forest::widget_id::WidgetId;
use crate::layout::axis::Axis;
use crate::layout::cache::{MeasureCache, SubtreeArenas, quantize_available};
use crate::layout::grid::GridContext;
use crate::layout::intrinsic::{LenReq, SLOT_COUNT};
use crate::layout::scroll::ScrollStates;
use crate::layout::stack::StackScratch;
use crate::layout::support::{AxisCtx, leaf_text_shapes, resolve_axis_size, zero_subtree};
use crate::layout::types::sizing::Sizing;
use crate::layout::types::span::Span;
use crate::layout::wrapstack::WrapScratch;
use crate::layout::{
    Layout, ShapedText, canvas, grid, intrinsic, scroll, stack, wrapstack, zstack,
};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::TextWrap;
use crate::text::TextShaper;
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
#[derive(Default)]
pub(crate) struct LayoutScratch {
    pub(crate) grid: GridContext,
    pub(crate) wrap: WrapScratch,
    pub(crate) stack_fill: StackScratch,
    pub(crate) desired: Vec<Size>,
    pub(crate) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(crate) tmp_hugs: Vec<f32>,
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
/// - `result` — per-frame output, one [`LayerLayout`] slot per `Layer`,
///   indexed via `result[layer]`. Internal measure/arrange code
///   reads/writes `self.result[self.active_layer]`; outside `run` every
///   slot is the finalized output for its layer. Read by the encoder,
///   cascade, hit-index, scroll-state refresh, and tests.
/// - `active_layer` — which layer's slot the recursive measure/arrange
///   currently writes to. Set at the top of each iteration in `run`;
///   between/outside `run` invocations its value is whatever the last
///   iteration left, but no recursive code runs there to read it.
/// - `cache` — cross-frame measure cache. See [`cache`] and
///   `src/layout/measure-cache.md`.
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

/// Quantize wrap target to ~0.1 logical px. Coarse enough to absorb
/// sub-pixel jitter from animated parents, fine enough that any
/// noticeable layout shift forces a reshape. Layout policy — lives
/// here, not in `text/`, so the granularity tradeoff is local to its
/// only consumer.
#[inline]
fn quantize_wrap_target(v: f32) -> u32 {
    (v.max(0.0) * 10.0).round() as u32
}

/// Fold a driver's raw `content` size into a margin-inclusive `desired`,
/// applying `style.size` (Fixed/Hug/Fill), padding, margin, and the
/// node's `[min, max]` clamp. Pulled out of `measure_dispatch` so the
/// grow-detection path can resolve once per dispatch without re-reading
/// `extras` between passes.
#[inline]
fn resolve_desired(
    style: LayoutCore,
    content: Size,
    available: Size,
    intrinsic_min: Size,
    min_size: Size,
    max_size: Size,
) -> Size {
    Size::new(
        resolve_axis_size(AxisCtx {
            sizing: style.size.w,
            hug_with_margin: content.w + style.padding.horiz() + style.margin.horiz(),
            available: available.w,
            intrinsic_min: intrinsic_min.w,
            margin: style.margin.horiz(),
            min: min_size.w,
            max: max_size.w,
        }),
        resolve_axis_size(AxisCtx {
            sizing: style.size.h,
            hug_with_margin: content.h + style.padding.vert() + style.margin.vert(),
            available: available.h,
            intrinsic_min: intrinsic_min.h,
            margin: style.margin.vert(),
            min: min_size.h,
            max: max_size.h,
        }),
    )
}

impl LayoutEngine {
    /// Drop cross-frame measure-cache entries and scroll-state rows for
    /// `WidgetId`s that vanished this frame. Called from `Ui::post_record`
    /// with the same `removed` slice that `Damage` and `TextShaper`
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
        text: &TextShaper,
    ) -> f32 {
        let slot = req.slot(axis);
        let cached = self.scratch.intrinsics[node.index()][slot];
        if !cached.is_nan() {
            return cached;
        }
        let v = intrinsic::compute(self, tree, node, axis, req, text);
        self.scratch.intrinsics[node.index()][slot] = v;
        v
    }

    /// Run measure + arrange for every root in every layer's tree
    /// against `surface` (the viewport rect). Iterates trees in
    /// `Layer::PAINT_ORDER`; each tree's output lands in
    /// `self.result[layer]` directly. Recursive measure/arrange reads
    /// the active slot via `self.active_layer`.
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        surface: Rect,
        text: &TextShaper,
        out: &mut Layout,
    ) {
        assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        let surface_end = surface.min + glam::Vec2::new(surface.size.w, surface.size.h);
        for layer in Layer::PAINT_ORDER {
            let tree = forest.tree(layer);
            self.active_layer = layer;
            out[layer].resize_for(tree);
            if tree.records.is_empty() {
                continue;
            }
            self.scratch.resize_for(tree);
            for slot in &tree.roots {
                let root = NodeId(slot.first_node);
                // Main: implicit root paints the full surface (Fill
                // semantic; arrange uses `available.max(desired)` so
                // overflow grows past it). Side layers: `anchor` is a
                // placement and `slot.size` is an optional cap; both
                // axes clamp to the surface bottom-right so an
                // oversized cap can't bleed past the viewport. The
                // root's own `Sizing` governs the painted size within
                // that available.
                let (origin, available) = if layer == Layer::Main {
                    (surface.min, surface.size)
                } else {
                    let rem = (surface_end - slot.anchor).max(glam::Vec2::ZERO);
                    let avail_w = slot.size.map_or(rem.x, |s| s.w.min(rem.x));
                    let avail_h = slot.size.map_or(rem.y, |s| s.h.min(rem.y));
                    (slot.anchor, Size::new(avail_w, avail_h))
                };
                let desired = self.measure(tree, root, available, text, out);
                let size = if layer == Layer::Main {
                    available.max(desired)
                } else {
                    desired
                };
                self.arrange(tree, root, Rect { min: origin, size }, out);
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
        text: &TextShaper,
        out: &mut Layout,
    ) -> Size {
        let style = tree.records.layout()[node.index()];
        if style.visibility.is_collapsed() {
            self.scratch.desired[node.index()] = Size::ZERO;
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
        let cache_wid = tree.records.widget_id()[node.index()];
        let cache_hash = tree.rollups.subtree[node.index()];
        let cache_avail = quantize_available(available);
        if let Some(hit) = self.cache.try_lookup(cache_wid, cache_hash, cache_avail) {
            let curr_start = node.index();
            let curr_end = curr_start + hit.arenas.desired.len();
            // Subtree hash includes child count + per-child rollups,
            // so a length mismatch here would mean the rollup is broken.
            assert_eq!(curr_end, (tree.records.subtree_end()[curr_start]) as usize);
            self.scratch.desired[curr_start..curr_end].copy_from_slice(hit.arenas.desired);
            // Append the snapshot's flat text-shape range to the live
            // per-frame buffer, then rebase its subtree-local spans by
            // `dest_start` into the per-node `text_spans` column.
            let dest_start = out[self.active_layer].text_shapes.len() as u32;
            out[self.active_layer]
                .text_shapes
                .extend_from_slice(hit.arenas.text_shapes);
            for (i, snap_span) in hit.arenas.text_spans.iter().enumerate() {
                out[self.active_layer].text_spans[curr_start + i] = Span {
                    start: dest_start + snap_span.start,
                    len: snap_span.len,
                };
            }
            // Restore per-grid hug arrays. `grid::arrange` reads
            // `LayoutEngine.scratch.grid.hugs`, populated only by
            // `grid::measure`. Without this restore, a cache hit at
            // any ancestor of a Grid leaves hugs zeroed and the
            // grid would collapse every cell to (0, 0). Pinned by
            // `widgets::tests::grid_cells_arranged_correctly_on_cache_hit_frame`.
            if tree.rollups.has_grid.contains(curr_start) {
                self.scratch
                    .grid
                    .hugs
                    .restore_subtree(tree, curr_start..curr_end, hit.arenas.hugs);
            }
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
        let intrinsic_min = Size::new(
            self.intrinsic(tree, node, Axis::X, LenReq::MinContent, text),
            self.intrinsic(tree, node, Axis::Y, LenReq::MinContent, text),
        );

        // Floor `available` at `intrinsic_min` before dispatch: the
        // node's actual outer size is `max(available, intrinsic_min)`
        // (per `resolve_axis_size`), so children must measure against
        // *that* width, not the smaller surface-derived value the
        // parent passed down. Without this, a Hug grid inside a
        // FILL panel whose own intrinsic_min is pinned by a long
        // sibling (e.g. a non-wrapping section title) keeps shrinking
        // with the surface even after the panel itself has stopped —
        // the panel arranges at its floor, but its measure dispatch
        // shaped the grid against the smaller surface width. Pinned
        // by `text_wrap::two_hug_cols_nonwrapping_label_floors_at_full_width`.
        //
        // Finite-only: `INFINITY.max(intrinsic_min)` is a no-op for
        // unbounded axes (Hug parent), preserving the WPF intrinsic
        // trick. Fixed axes ignore both inputs in `resolve_axis_size`,
        // so flooring here is harmless for them.
        let dispatch_avail = Size::new(
            available.w.max(intrinsic_min.w),
            available.h.max(intrinsic_min.h),
        );

        // Single dispatch. When `desired` exceeds `available` on a
        // non-Fixed axis it's because a rigid descendant pinned the
        // floor (`intrinsic_min` / `min_size` / `Sizing::Fixed`); a
        // re-dispatch against the grown outer would converge to the
        // same value because every driver's content size is monotone
        // in `available` and pass-1 already saturated at the floor.
        // Pinned by `cross_driver_tests::convergence`.
        let content = self.measure_dispatch(tree, node, style, dispatch_avail, text, out);
        let desired = resolve_desired(style, content, available, intrinsic_min, min_size, max_size);

        self.scratch.desired[node.index()] = desired;

        // Snapshot the entire subtree we just (re)measured. Pre-order
        // arena means the subtree is `[node.index() .. subtree_end[i]]`
        // contiguous in both `desired` and `text_shapes`. Capacity
        // retains across frames via `clear() + extend_from_slice`
        // inside `MeasureCache::write_subtree`. Per-grid hug arrays
        // for descendant Grids land in `scratch.tmp_hugs` first;
        // empty for grid-free subtrees.
        {
            let start = node.index();
            let end = (tree.records.subtree_end()[start]) as usize;
            self.scratch.tmp_hugs.clear();
            if tree.rollups.has_grid.contains(start) {
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
            self.cache.write_subtree(
                cache_wid,
                cache_hash,
                cache_avail,
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

    /// One measure pass for `node`: derives `inner_avail` from
    /// `available` and `style`, dispatches to the driver, returns the
    /// driver's raw content size. Called twice from `measure` when a
    /// Fill axis grows past `available` so the children see their
    /// actual post-grow inner. The caller folds content into a
    /// margin-inclusive `desired` via `resolve_desired`.
    ///
    /// ## Driver contract
    ///
    /// Every layout driver (`stack`, `wrapstack`, `zstack`, `canvas`,
    /// `grid`) is a free module exporting three `pub(crate) fn`s,
    /// matched into here and into [`Self::arrange`] / `intrinsic::compute`:
    ///
    /// - `measure(layout, tree, node, [variant_payload,] inner_avail, text) -> Size`
    ///   — bottom-up. Recurses into children via `layout.measure(...)`.
    ///   Returns the driver's content size (pre-padding/margin/clamp;
    ///   the caller in [`Self::measure`] folds those in).
    /// - `arrange(layout, tree, node, [variant_payload,] inner)`
    ///   — top-down. Assigns each child a final rect and recurses via
    ///   `layout.arrange(...)`.
    /// - `intrinsic(layout, tree, node, [variant_payload,] axis, req, text) -> f32`
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
        available: Size,
        text: &TextShaper,
        out: &mut Layout,
    ) -> Size {
        // For each axis: if this node has a declared `Fixed` size, that's the
        // outer width children see — `inner = fixed - padding`. Otherwise
        // (Hug / Fill) we propagate whatever the parent gave us. Without
        // this, a fixed-width parent above a wrapping child wouldn't
        // constrain the child's available width during measure, so wrapping
        // text would never reshape.
        //
        // Also clamp by this node's own `min_size`/`max_size` so a
        // capped parent doesn't grant children more room than it can
        // arrange. `resolve_axis_size` applies the same clamp to the
        // outer size; doing it here too keeps children's `available`
        // consistent with the parent's eventual arranged width — Fill
        // children inside a `max_size`-capped parent see the capped
        // budget instead of bleeding past the parent's edge.
        let bounds = tree.bounds(node);
        let outer_w = match style.size.w {
            Sizing::Fixed(v) => v,
            _ => (available.w - style.margin.horiz()).max(0.0),
        }
        .clamp(bounds.min_size.w, bounds.max_size.w);
        let outer_h = match style.size.h {
            Sizing::Fixed(v) => v,
            _ => (available.h - style.margin.vert()).max(0.0),
        }
        .clamp(bounds.min_size.h, bounds.max_size.h);
        let inner_avail = Size::new(
            (outer_w - style.padding.horiz()).max(0.0),
            (outer_h - style.padding.vert()).max(0.0),
        );

        match style.mode {
            LayoutMode::Leaf => self.leaf_content_size(tree, node, inner_avail.w, text, out),
            LayoutMode::HStack => stack::measure(self, tree, node, inner_avail, Axis::X, text, out),
            LayoutMode::VStack => stack::measure(self, tree, node, inner_avail, Axis::Y, text, out),
            LayoutMode::WrapHStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::X, text, out)
            }
            LayoutMode::WrapVStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::Y, text, out)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, inner_avail, text, out),
            LayoutMode::Canvas => canvas::measure(self, tree, node, inner_avail, text, out),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx, inner_avail, text, out),
            // Scroll viewport. INF-axis measure of children; the
            // driver also writes the panned-axis content extent into
            // the persistent `ScrollLayoutState` row (see
            // `scroll::measure`).
            LayoutMode::Scroll(axes) => {
                scroll::measure(self, tree, node, inner_avail, axes, text, out)
            }
        }
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in the
    /// active layer's `Layout`.
    pub(crate) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect, out: &mut Layout) {
        let style = tree.records.layout()[node.index()];
        if style.visibility.is_collapsed() {
            zero_subtree(self, tree, node, slot.min, out);
            return;
        }
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        out[self.active_layer].rect[node.index()] = rendered;
        let inner = rendered.deflated_by(style.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, Axis::X, out),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, Axis::Y, out),
            LayoutMode::WrapHStack => wrapstack::arrange(self, tree, node, inner, Axis::X, out),
            LayoutMode::WrapVStack => wrapstack::arrange(self, tree, node, inner, Axis::Y, out),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner, out),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner, out),
            LayoutMode::Grid(idx) => grid::arrange(self, tree, node, inner, idx, out),
            LayoutMode::Scroll(axes) => scroll::arrange(self, tree, node, inner, axes, out),
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
        text: &TextShaper,
        out: &mut Layout,
    ) -> Size {
        let span_start = out[self.active_layer].text_shapes.len() as u32;
        let mut s = Size::ZERO;
        let mut ordinal: u16 = 0;
        for ts in leaf_text_shapes(tree, node) {
            let m = self.shape_text(
                tree,
                node,
                ordinal,
                ts.text,
                ts.font_size_px,
                ts.line_height_px,
                ts.wrap,
                available_w,
                text,
                out,
            );
            s = s.max(m);
            ordinal = ordinal.checked_add(1).expect(
                "more than 65535 ShapeRecord::Text per leaf — well past anything sane; \
                 widen the within-node ordinal width if this trips",
            );
        }
        let span_len = out[self.active_layer].text_shapes.len() as u32 - span_start;
        out[self.active_layer].text_spans[node.index()] = Span {
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
        src: &str,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        available_w: f32,
        text: &TextShaper,
        out: &mut Layout,
    ) -> Size {
        let wid = tree.records.widget_id()[node.index()];
        let curr_hash = tree.rollups.node[node.index()];

        // Refresh the unbounded measurement only when the authoring hash
        // has shifted. Crucially, when only the wrap target changed
        // (e.g. animated parent width), the unbounded cache is
        // preserved and only the wrap reshape runs in shape_wrap.
        let unbounded =
            text.shape_unbounded(wid, ordinal, curr_hash, src, font_size_px, line_height_px);

        let want_wrap = matches!(wrap, TextWrap::Wrap)
            && available_w.is_finite()
            && available_w < unbounded.size.w;

        let result = if want_wrap {
            let target = available_w.max(unbounded.intrinsic_min);
            let target_q = quantize_wrap_target(target);
            text.shape_wrap(
                wid,
                ordinal,
                src,
                font_size_px,
                line_height_px,
                target,
                target_q,
            )
        } else {
            unbounded
        };

        out[self.active_layer].text_shapes.push(ShapedText {
            measured: result.size,
            key: result.key,
        });
        result.size
    }
}
