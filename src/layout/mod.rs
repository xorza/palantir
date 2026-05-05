use crate::layout::axis::Axis;
use crate::layout::cache::{MeasureCache, quantize_available};
use crate::layout::grid::GridContext;
use crate::layout::intrinsic::{LenReq, SLOT_COUNT};
use crate::layout::result::{LayoutResult, ShapedText};
use crate::layout::support::{leaf_text_shapes, resolve_axis_size, zero_subtree};
use crate::layout::types::sizing::Sizing;
use crate::layout::wrapstack::WrapScratch;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::TextWrap;
use crate::text::TextMeasurer;
use crate::tree::element::{LayoutCore, LayoutMode, ScrollAxes};
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree};

pub(crate) mod axis;
pub(crate) mod cache;
pub(crate) mod canvas;
pub(crate) mod grid;
pub(crate) mod intrinsic;
pub(crate) mod result;
pub(crate) mod stack;
pub(crate) mod support;
pub(crate) mod types;
pub(crate) mod wrapstack;
pub(crate) mod zstack;

#[cfg(test)]
mod cross_driver_tests;

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
    pub(crate) desired: Vec<Size>,
    pub(crate) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(crate) tmp_hugs: Vec<f32>,
}

impl LayoutScratch {
    fn resize_for(&mut self, tree: &Tree) {
        let n = tree.layout.len();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.intrinsics.clear();
        self.intrinsics.resize(n, [f32::NAN; SLOT_COUNT]);
        self.grid.hugs.reset_for(tree);
    }
}

/// Persistent layout engine. Three field groups, each with its own
/// lifetime:
///
/// - `scratch` — per-frame intermediate state (see [`LayoutScratch`]).
///   Cleared at the top of every `run`.
/// - `result` — per-frame output (rects + text shapes). Read by
///   encoder / hit-index after `run` returns. Exposed via
///   [`LayoutEngine::result`].
/// - `cache` — cross-frame measure cache. See [`cache`] and
///   `src/layout/measure-cache.md`.
///
/// Cross-frame text reuse used to live here too; it now sits behind
/// `TextMeasurer` (`unbounded_for` / `cached_wrap` / `shape_wrap`) so
/// the dispatch-skip and the cache live in one place.
#[derive(Default)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(crate) result: LayoutResult,
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
        resolve_axis_size(
            style.size.w,
            content.w + style.padding.horiz() + style.margin.horiz(),
            available.w,
            intrinsic_min.w,
            style.margin.horiz(),
            min_size.w,
            max_size.w,
        ),
        resolve_axis_size(
            style.size.h,
            content.h + style.padding.vert() + style.margin.vert(),
            available.h,
            intrinsic_min.h,
            style.margin.vert(),
            min_size.h,
            max_size.h,
        ),
    )
}

impl LayoutEngine {
    /// Drop cross-frame measure-cache entries for `WidgetId`s that
    /// vanished this frame. Called from `Ui::end_frame` with the same
    /// `removed` slice that `Damage` and `TextMeasurer` consume.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.cache.sweep_removed(removed);
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
        text: &mut TextMeasurer,
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

    /// Run measure + arrange for `root` given the surface rect. Reuses
    /// internal scratch — call this each frame for amortized zero-alloc
    /// layout (after warmup). Output lands in `self.result`.
    ///
    /// `text` carries the shaper (or the mono fallback inside it) and is
    /// borrowed for the duration of the call so wrapping leaves can reshape
    /// against the parent-committed width during measure.
    pub(crate) fn run(
        &mut self,
        tree: &Tree,
        root: Option<NodeId>,
        surface: Rect,
        text: &mut TextMeasurer,
    ) -> &LayoutResult {
        assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        self.scratch.resize_for(tree);
        self.result.resize_for(tree);
        // No root ⇒ no widgets recorded this frame. Result is sized to
        // `tree.layout.len() == 0`, so downstream consumers walk zero
        // entries — return the freshly-cleared result without measuring.
        if let Some(root) = root {
            // Root slot grows past the surface when measured content
            // exceeds it, so the parent-≥-child invariant from
            // `resolve_axis_size` (Fill/Hug ≥ hug_with_margin) holds
            // at the root too. Downstream (cascade/composer/backend)
            // tolerates out-of-surface rects; the GPU scissor clips at
            // the viewport. `Fixed` is unaffected: it short-circuits
            // in `resolve_axis_size` and never reads `hug_with_margin`.
            let desired = self.measure(tree, root, surface.size, text);
            let slot = Rect {
                min: surface.min,
                size: surface.size.max(desired),
            };
            self.arrange(tree, root, slot);
        }
        assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
        &self.result
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores the resolved size for each visited node in
    /// `self.desired` (read by `arrange`).
    pub(crate) fn measure(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available: Size,
        text: &mut TextMeasurer,
    ) -> Size {
        if tree.is_collapsed(node) {
            self.scratch.desired[node.index()] = Size::ZERO;
            return Size::ZERO;
        }
        let style = tree.layout[node.index()];

        // Phase-2 measure-cache short-circuit: any node. Same
        // `WidgetId`, same rolled subtree hash, same quantized
        // `available` → restore the *whole subtree*'s `desired` and
        // text shapes from last frame's snapshot and skip recursion
        // entirely. The subtree-hash rollup guarantees structural and
        // authoring equivalence; `available_q` guards against parent
        // resize since outer-leaf measure is `available`-dependent
        // for `Hug` / `Fill` axes.
        let cache_wid = tree.widget_ids[node.index()];
        let cache_hash = tree.hashes.subtree[node.index()];
        let cache_avail = quantize_available(available);
        // Record this node's quantized `available` before any
        // short-circuit. Downstream consumers (encode cache, etc.)
        // read the column at every node they visit; on a measure-cache
        // hit the descendant range is restored from the snapshot
        // below, so this single write covers the miss path and the
        // snapshot covers the hit path.
        self.result.available_q[node.index()] = cache_avail;
        if let Some(hit) = self.cache.try_lookup(cache_wid, cache_hash, cache_avail) {
            let curr_start = node.index();
            let curr_end = curr_start + hit.desired.len();
            // Subtree hash includes child count + per-child rollups,
            // so a length mismatch here would mean the rollup is broken.
            assert_eq!(curr_end, tree.subtree_end[curr_start] as usize);
            self.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
            self.result.text_shapes[curr_start..curr_start + hit.text_shapes.len()]
                .copy_from_slice(hit.text_shapes);
            self.result.available_q[curr_start..curr_end].copy_from_slice(hit.available_q);
            self.result.scroll_content[curr_start..curr_end].copy_from_slice(hit.scroll_content);
            // Restore per-grid hug arrays. `grid::arrange` reads
            // `LayoutEngine.scratch.grid.hugs`, populated only by
            // `grid::measure`. Without this restore, a cache hit at
            // any ancestor of a Grid leaves hugs zeroed and the
            // grid would collapse every cell to (0, 0). Pinned by
            // `widgets::tests::grid_cells_arranged_correctly_on_cache_hit_frame`.
            if tree.hashes.subtree_has_grid[curr_start] {
                self.scratch
                    .grid
                    .hugs
                    .restore_subtree(tree, curr_start..curr_end, hit.hugs);
            }
            return hit.root;
        }

        let extras = tree.read_extras(node);
        let (min_size, max_size) = (extras.min_size, extras.max_size);

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

        // First dispatch: children see `inner_avail` derived from the
        // parent-passed `available`. May grow on a Fill axis when a
        // Fixed/Hug descendant's hug exceeds `available`.
        let content = self.measure_dispatch(tree, node, style, available, text);
        let desired = resolve_desired(style, content, available, intrinsic_min, min_size, max_size);

        // Re-dispatch when grow happened on an axis whose children's
        // `inner_avail` actually depends on `available`. Pass 1 measured
        // children against the pre-grow `inner_avail` so they don't
        // know they'll be allocated more space at arrange time —
        // wrapstacks would pack extra rows, etc. Pass 2 re-measures
        // with the grown outer so children see their actual post-grow
        // inner. Recursion propagates: descendants that also grew
        // re-dispatch inside the call. Converges in one extra dispatch
        // because children of a grown parent are bounded by the new
        // larger inner, so `desired` can only stay the same or shrink
        // — never exceed `new_available` to trigger a third pass.
        //
        // `Sizing::Fixed` short-circuits: its `outer` doesn't read
        // `available`, so pass 2 would produce identical `inner_avail`
        // and is pure waste.
        //
        // Cost: O(grown subtree) on frames a grow happens; the measure
        // cache absorbs unaffected descendants on subsequent frames.
        let grew_w = available.w.is_finite()
            && desired.w > available.w
            && !matches!(style.size.w, Sizing::Fixed(_));
        let grew_h = available.h.is_finite()
            && desired.h > available.h
            && !matches!(style.size.h, Sizing::Fixed(_));
        let new_available = Size::new(
            if grew_w { desired.w } else { available.w },
            if grew_h { desired.h } else { available.h },
        );
        let desired = if grew_w || grew_h {
            let content = self.measure_dispatch(tree, node, style, new_available, text);
            let final_desired = resolve_desired(
                style,
                content,
                new_available,
                intrinsic_min,
                min_size,
                max_size,
            );
            // Non-monotonic layouts (wrap-stacks; Fill distributions
            // where a descendant's hug grows when given more space)
            // can produce `final_desired > new_available` even after a
            // second pass. The original convergence guarantee assumed
            // monotonicity which doesn't hold there. Clamp to the
            // parent's already-committed slot — any overflow renders
            // inside and is tolerated by downstream
            // (cascade/composer/backend), same posture the run loop
            // takes for root-vs-surface overflow. Pinned by
            // `cross_driver_tests::convergence`.
            Size::new(
                final_desired.w.min(new_available.w),
                final_desired.h.min(new_available.h),
            )
        } else {
            desired
        };

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
            let end = tree.subtree_end[start] as usize;
            self.scratch.tmp_hugs.clear();
            if tree.hashes.subtree_has_grid[start] {
                self.scratch.grid.hugs.snapshot_subtree(
                    tree,
                    start..end,
                    &mut self.scratch.tmp_hugs,
                );
            }
            self.cache.write_subtree(
                cache_wid,
                cache_hash,
                &self.scratch.desired[start..end],
                &self.result.text_shapes[start..end],
                &self.result.available_q[start..end],
                &self.result.scroll_content[start..end],
                &self.scratch.tmp_hugs,
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
        text: &mut TextMeasurer,
    ) -> Size {
        // For each axis: if this node has a declared `Fixed` size, that's the
        // outer width children see — `inner = fixed - padding`. Otherwise
        // (Hug / Fill) we propagate whatever the parent gave us. Without
        // this, a fixed-width parent above a wrapping child wouldn't
        // constrain the child's available width during measure, so wrapping
        // text would never reshape.
        let outer_w = match style.size.w {
            Sizing::Fixed(v) => v,
            _ => (available.w - style.margin.horiz()).max(0.0),
        };
        let outer_h = match style.size.h {
            Sizing::Fixed(v) => v,
            _ => (available.h - style.margin.vert()).max(0.0),
        };
        let inner_avail = Size::new(
            (outer_w - style.padding.horiz()).max(0.0),
            (outer_h - style.padding.vert()).max(0.0),
        );

        match style.mode {
            LayoutMode::Leaf => self.leaf_content_size(tree, node, inner_avail.w, text),
            LayoutMode::HStack => stack::measure(self, tree, node, inner_avail, Axis::X, text),
            LayoutMode::VStack => stack::measure(self, tree, node, inner_avail, Axis::Y, text),
            LayoutMode::WrapHStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::X, text)
            }
            LayoutMode::WrapVStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::Y, text)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, inner_avail, text),
            LayoutMode::Canvas => canvas::measure(self, tree, node, inner_avail, text),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx, inner_avail, text),
            // Scroll viewports stash content extent in `scroll_content`
            // for `Ui::end_frame` and return 0 on the panned axes so
            // `resolve_desired` falls through to the user's `Sizing`
            // and doesn't grow with content. Single-axis variants run
            // a stack on the panned axis with that axis fed `INF`;
            // `Both` runs a zstack with both axes unbounded.
            LayoutMode::Scroll(axes) => {
                let raw = match axes {
                    ScrollAxes::Vertical => stack::measure(
                        self,
                        tree,
                        node,
                        Size::new(inner_avail.w, f32::INFINITY),
                        Axis::Y,
                        text,
                    ),
                    ScrollAxes::Horizontal => stack::measure(
                        self,
                        tree,
                        node,
                        Size::new(f32::INFINITY, inner_avail.h),
                        Axis::X,
                        text,
                    ),
                    ScrollAxes::Both => zstack::measure(self, tree, node, Size::INF, text),
                };
                self.result.scroll_content[node.index()] = raw;
                match axes {
                    ScrollAxes::Vertical => Size::new(raw.w, 0.0),
                    ScrollAxes::Horizontal => Size::new(0.0, raw.h),
                    ScrollAxes::Both => Size::ZERO,
                }
            }
        }
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in `self.result`.
    pub(crate) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect) {
        if tree.is_collapsed(node) {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
        let style = tree.layout[node.index()];
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        self.result.rect[node.index()] = rendered;
        let inner = rendered.deflated_by(style.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, Axis::X),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, Axis::Y),
            LayoutMode::WrapHStack => wrapstack::arrange(self, tree, node, inner, Axis::X),
            LayoutMode::WrapVStack => wrapstack::arrange(self, tree, node, inner, Axis::Y),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner),
            LayoutMode::Grid(idx) => grid::arrange(self, tree, node, inner, idx),
            LayoutMode::Scroll(axes) => match axes {
                ScrollAxes::Vertical => stack::arrange(self, tree, node, inner, Axis::Y),
                ScrollAxes::Horizontal => stack::arrange(self, tree, node, inner, Axis::X),
                ScrollAxes::Both => zstack::arrange(self, tree, node, inner),
            },
        }
    }

    /// Walk a Leaf's recorded shapes and return the content size that drives
    /// its hugging. For `Shape::Text` runs, this is also where shaping
    /// happens: the shaped buffer + measured size land on
    /// `LayoutResult.text_shapes` so the encoder can pick them up later.
    /// `available_w` flows down from the parent and gates wrapping.
    fn leaf_content_size(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let mut s = Size::ZERO;
        for ts in leaf_text_shapes(tree, node) {
            let m = self.shape_text(
                tree,
                node,
                ts.text,
                ts.font_size_px,
                ts.line_height_px,
                ts.wrap,
                available_w,
                text,
            );
            s = s.max(m);
        }
        s
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_text(
        &mut self,
        tree: &Tree,
        node: NodeId,
        src: &str,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let wid = tree.widget_ids[node.index()];
        let curr_hash = tree.hashes.node[node.index()];

        // Refresh the unbounded measurement only when the authoring hash
        // has shifted. Crucially, when only the wrap target changed
        // (e.g. animated parent width), the unbounded cache is
        // preserved and only the wrap reshape runs in shape_wrap.
        let unbounded = text.shape_unbounded(wid, curr_hash, src, font_size_px, line_height_px);

        let want_wrap = matches!(wrap, TextWrap::Wrap)
            && available_w.is_finite()
            && available_w < unbounded.size.w;

        let result = if want_wrap {
            let target = available_w.max(unbounded.intrinsic_min);
            let target_q = quantize_wrap_target(target);
            text.shape_wrap(wid, src, font_size_px, line_height_px, target, target_q)
        } else {
            unbounded
        };

        self.result.text_shapes[node.index()] = Some(ShapedText {
            measured: result.size,
            key: result.key,
        });
        result.size
    }
}
