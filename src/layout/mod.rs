use crate::layout::axis::Axis;
use crate::layout::cache::{MeasureCache, SubtreeArenas, quantize_available};
use crate::layout::grid::GridContext;
use crate::layout::intrinsic::{LenReq, SLOT_COUNT};
use crate::layout::result::{LayoutResult, ShapedText};
use crate::layout::stack::StackScratch;
use crate::layout::support::{AxisCtx, leaf_text_shapes, resolve_axis_size, zero_subtree};
use crate::layout::types::sizing::Sizing;
use crate::layout::types::span::Span;
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
    pub(crate) stack_fill: StackScratch,
    pub(crate) desired: Vec<Size>,
    pub(crate) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(crate) tmp_hugs: Vec<f32>,
    /// Staging buffer for rebasing per-node `text_spans` to subtree-
    /// local form before writing a `MeasureCache` snapshot. Reused
    /// across snapshots in one frame; capacity retained across frames.
    pub(crate) tmp_text_spans: Vec<Span>,
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
        // `tree.records.len() == 0`, so downstream consumers walk zero
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
            let curr_end = curr_start + hit.arenas.desired.len();
            // Subtree hash includes child count + per-child rollups,
            // so a length mismatch here would mean the rollup is broken.
            assert_eq!(curr_end, (tree.records.end()[curr_start]) as usize);
            self.scratch.desired[curr_start..curr_end].copy_from_slice(hit.arenas.desired);
            // Append the snapshot's flat text-shape range to the live
            // per-frame buffer, then rebase its subtree-local spans by
            // `dest_start` into the per-node `text_spans` column.
            let dest_start = self.result.text_shapes.len() as u32;
            self.result
                .text_shapes
                .extend_from_slice(hit.arenas.text_shapes);
            for (i, snap_span) in hit.arenas.text_spans.iter().enumerate() {
                self.result.text_spans[curr_start + i] = Span {
                    start: dest_start + snap_span.start,
                    len: snap_span.len,
                };
            }
            self.result.available_q[curr_start..curr_end].copy_from_slice(hit.arenas.available_q);
            self.result.scroll_content[curr_start..curr_end]
                .copy_from_slice(hit.arenas.scroll_content);
            // Restore per-grid hug arrays. `grid::arrange` reads
            // `LayoutEngine.scratch.grid.hugs`, populated only by
            // `grid::measure`. Without this restore, a cache hit at
            // any ancestor of a Grid leaves hugs zeroed and the
            // grid would collapse every cell to (0, 0). Pinned by
            // `widgets::tests::grid_cells_arranged_correctly_on_cache_hit_frame`.
            if tree.subtree_has_grid.contains(curr_start) {
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
        let text_shapes_lo = self.result.text_shapes.len() as u32;

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
        let content = self.measure_dispatch(tree, node, style, dispatch_avail, text);
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
            let end = (tree.records.end()[start]) as usize;
            // The snapshot's root `available_q` must match the value
            // written at measure entry; cache hits restore it back into
            // `result.available_q` and downstream caches key on it.
            assert_eq!(self.result.available_q[start], cache_avail);
            self.scratch.tmp_hugs.clear();
            if tree.subtree_has_grid.contains(start) {
                self.scratch.grid.hugs.snapshot_subtree(
                    tree,
                    start..end,
                    &mut self.scratch.tmp_hugs,
                );
            }
            // Rebase per-node spans to subtree-local form (start
            // relative to `text_shapes_lo`) so the snapshot remains
            // valid after compaction relocates the flat range. Empty
            // spans (`Span::default()` with start=0) round-trip through
            // `saturating_sub` correctly: 0 - lo = 0.
            let text_shapes_hi = self.result.text_shapes.len() as u32;
            self.scratch.tmp_text_spans.clear();
            self.scratch
                .tmp_text_spans
                .extend(self.result.text_spans[start..end].iter().map(|s| Span {
                    start: s.start.saturating_sub(text_shapes_lo),
                    len: s.len,
                }));
            self.cache.write_subtree(
                cache_wid,
                cache_hash,
                SubtreeArenas {
                    desired: &self.scratch.desired[start..end],
                    text_spans: &self.scratch.tmp_text_spans,
                    available_q: &self.result.available_q[start..end],
                    scroll_content: &self.result.scroll_content[start..end],
                    hugs: &self.scratch.tmp_hugs,
                    text_shapes: &self.result.text_shapes
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
        let style = tree.records.layout()[node.index()];
        if style.visibility.is_collapsed() {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
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
        let span_start = self.result.text_shapes.len() as u32;
        let mut s = Size::ZERO;
        let mut ordinal: u8 = 0;
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
            );
            s = s.max(m);
            ordinal = ordinal.checked_add(1).expect(
                "more than 255 Shape::Text per leaf — well past anything sane; \
                 widen the within-node ordinal width if this trips",
            );
        }
        let span_len = self.result.text_shapes.len() as u32 - span_start;
        self.result.text_spans[node.index()] = Span {
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
        ordinal: u8,
        src: &str,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let wid = tree.records.widget_id()[node.index()];
        let curr_hash = tree.hashes.node[node.index()];

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

        self.result.text_shapes.push(ShapedText {
            measured: result.size,
            key: result.key,
        });
        result.size
    }
}
