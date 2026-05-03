use crate::element::LayoutMode;
use crate::primitives::{Rect, Size, Sizing, WidgetId};
use crate::shape::{Shape, TextWrap};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};
use cache::MeasureCache;
use grid::GridContext;
use support::{resolve_axis_size, zero_subtree};
use wrapstack::WrapScratch;

mod axis;
mod cache;
mod canvas;
mod grid;
mod intrinsic;
mod result;
mod stack;
mod support;
mod wrapstack;
mod zstack;

#[cfg(test)]
mod integration_tests;

pub use axis::Axis;
pub use cache::{AvailableKey, quantize_available};
pub use intrinsic::LenReq;
pub use result::{LayoutResult, ShapedText};

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
/// - `available_q` — per-node quantized `available` size, the
///   dimensional half of the cross-frame cache key. Written on every
///   measure entry, restored from a snapshot on cache-hit subtrees.
///   Read by the encode cache (and any other consumer keyed on the
///   same `(subtree_hash, available_q)` shape as `MeasureCache`).
///
/// Module-internal tests (e.g. `stack/tests.rs`) reach in via
/// `pub(in crate::layout)` to pin measure output independently of
/// arrange's slot-clamping.
#[derive(Default)]
pub(in crate::layout) struct LayoutScratch {
    pub(in crate::layout) grid: GridContext,
    pub(in crate::layout) wrap: WrapScratch,
    pub(in crate::layout) desired: Vec<Size>,
    pub(in crate::layout) intrinsics: Vec<[f32; 4]>,
    pub(in crate::layout) available_q: Vec<AvailableKey>,
}

impl LayoutScratch {
    fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.intrinsics.clear();
        self.intrinsics.resize(n, [f32::NAN; 4]);
        self.available_q.clear();
        self.available_q.resize(n, AvailableKey::default());
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
///   `docs/measure-cache.md`.
///
/// Cross-frame text reuse used to live here too; it now sits behind
/// `TextMeasurer` (`unbounded_for` / `cached_wrap` / `shape_wrap`) so
/// the dispatch-skip and the cache live in one place.
#[derive(Default)]
pub struct LayoutEngine {
    pub(in crate::layout) scratch: LayoutScratch,
    pub(in crate::layout) result: LayoutResult,
    pub(in crate::layout) cache: MeasureCache,
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

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn result(&self) -> &LayoutResult {
        &self.result
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.result.rect(id)
    }

    /// Drop cross-frame measure-cache entries for `WidgetId`s that
    /// vanished this frame. Called from `Ui::end_frame` with the same
    /// `removed` slice that `Damage` and `TextMeasurer` consume.
    pub fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.cache.sweep_removed(removed);
    }

    /// Drop every cross-frame measure-cache entry. `#[doc(hidden)]` —
    /// see [`crate::Ui::__clear_measure_cache`].
    #[doc(hidden)]
    pub fn __clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Per-node quantized `available` size last passed to this node's
    /// measure. Read by the encode cache (and any other consumer keyed
    /// on the same `(subtree_hash, available_q)` shape as
    /// `MeasureCache`).
    #[inline]
    pub fn available_q(&self, id: NodeId) -> AvailableKey {
        self.scratch.available_q[id.index()]
    }

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` under content-sizing `req`. See `intrinsic.md`.
    ///
    /// Pure function of the subtree at `node`: doesn't depend on the
    /// parent's available width or the arranged rect. Memoized via the
    /// intra-frame cache so repeated queries during the same `run` cost
    /// one array load. Consumed by `grid::measure` (Phase 1 column
    /// resolution) and `stack::measure` (Fill min-content floor).
    pub fn intrinsic(
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
    pub fn run(
        &mut self,
        tree: &Tree,
        root: Option<NodeId>,
        surface: Rect,
        text: &mut TextMeasurer,
    ) -> &LayoutResult {
        assert_eq!(
            self.scratch.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        self.scratch.resize_for(tree);
        self.result.resize_for(tree);
        // No root ⇒ no widgets recorded this frame. Result is sized to
        // `tree.node_count() == 0`, so downstream consumers walk zero
        // entries — return the freshly-cleared result without measuring.
        if let Some(root) = root {
            self.measure(
                tree,
                root,
                Size::new(surface.width(), surface.height()),
                text,
            );
            self.arrange(tree, root, surface);
        }
        assert_eq!(
            self.scratch.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
        &self.result
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores the resolved size for each visited node in
    /// `self.desired` (read by `arrange`).
    pub(in crate::layout) fn measure(
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
        let style = *tree.layout(node);
        let mode = style.mode;
        let extras = tree.read_extras(node);
        let (min_size, max_size) = (extras.min_size, extras.max_size);

        // Phase-2 measure-cache short-circuit: any node. Same
        // `WidgetId`, same rolled subtree hash, same quantized
        // `available` → restore the *whole subtree*'s `desired` and
        // text shapes from last frame's snapshot and skip recursion
        // entirely. The subtree-hash rollup guarantees structural and
        // authoring equivalence; `available_q` guards against parent
        // resize since outer-leaf measure is `available`-dependent
        // for `Hug` / `Fill` axes.
        let cache_wid = tree.widget_ids[node.index()];
        let cache_hash = tree.subtree_hashes[node.index()];
        let cache_avail = quantize_available(available);
        // Record this node's quantized `available` before any
        // short-circuit. Downstream consumers (encode cache, etc.)
        // read the column at every node they visit; on a measure-cache
        // hit the descendant range is restored from the snapshot
        // below, so this single write covers the miss path and the
        // snapshot covers the hit path.
        self.scratch.available_q[node.index()] = cache_avail;
        if let Some(hit) = self.cache.try_lookup(cache_wid, cache_hash, cache_avail) {
            let curr_start = node.index();
            let curr_end = curr_start + hit.desired.len();
            // Subtree hash includes child count + per-child rollups,
            // so a length mismatch here would mean the rollup is broken.
            debug_assert_eq!(curr_end, tree.subtree_end[curr_start] as usize);
            self.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
            self.result.restore_text_shapes(curr_start, hit.text_shapes);
            self.scratch.available_q[curr_start..curr_end].copy_from_slice(hit.available_q);
            // Restore per-grid hug arrays. `grid::arrange` reads
            // `LayoutEngine.scratch.grid.hugs`, populated only by
            // `grid::measure`. Without this restore, a cache hit at
            // any ancestor of a Grid leaves hugs zeroed and the
            // grid would collapse every cell to (0, 0). Pinned by
            // `widgets::tests::grid_cells_arranged_correctly_on_cache_hit_frame`.
            if tree.subtree_has_grid[curr_start] {
                self.scratch
                    .grid
                    .hugs
                    .restore_subtree(tree, curr_start..curr_end, hit.hugs);
            }
            return hit.root;
        }

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

        let content = match mode {
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
        };

        let hug_w = content.w + style.padding.horiz() + style.margin.horiz();
        let hug_h = content.h + style.padding.vert() + style.margin.vert();
        let desired = Size::new(
            resolve_axis_size(
                style.size.w,
                hug_w,
                available.w,
                style.margin.horiz(),
                min_size.w,
                max_size.w,
            ),
            resolve_axis_size(
                style.size.h,
                hug_h,
                available.h,
                style.margin.vert(),
                min_size.h,
                max_size.h,
            ),
        );

        self.scratch.desired[node.index()] = desired;

        // Snapshot the entire subtree we just (re)measured. Pre-order
        // arena means the subtree is `[node.index() .. subtree_end[i]]`
        // contiguous in both `desired` and `text_shapes`. Capacity
        // retains across frames via `clear() + extend_from_slice`
        // inside `MeasureCache::write_subtree`. Per-grid hug arrays
        // for descendant Grids land in the cache's own
        // `tmp_hugs` buffer first; empty for grid-free subtrees.
        {
            let start = node.index();
            let end = tree.subtree_end[start] as usize;
            self.cache.tmp_hugs.clear();
            if tree.subtree_has_grid[start] {
                self.scratch
                    .grid
                    .hugs
                    .snapshot_subtree(tree, start..end, &mut self.cache.tmp_hugs);
            }
            self.cache.write_subtree(
                cache_wid,
                cache_hash,
                &self.scratch.desired[start..end],
                self.result.text_shapes_slice(start..end),
                &self.scratch.available_q[start..end],
            );
        }

        desired
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in `self.result`.
    pub(in crate::layout) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect) {
        if tree.is_collapsed(node) {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
        let style = *tree.layout(node);
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        self.result.set_rect(node, rendered);
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
        for shape in tree.shapes_of(node) {
            if let Shape::Text {
                text: src,
                font_size_px,
                wrap,
                ..
            } = shape
            {
                let m = self.shape_text(tree, node, src, *font_size_px, *wrap, available_w, text);
                s = s.max(m);
            }
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
        wrap: TextWrap,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let wid = tree.widget_ids[node.index()];
        let curr_hash = tree.hashes[node.index()];

        // Refresh the unbounded measurement only when the authoring hash
        // has shifted. Crucially, when only the wrap target changed
        // (e.g. animated parent width), the unbounded cache is
        // preserved and only the wrap reshape runs in shape_wrap.
        let unbounded = text.shape_unbounded(wid, curr_hash, src, font_size_px);

        let want_wrap = matches!(wrap, TextWrap::Wrap)
            && available_w.is_finite()
            && available_w < unbounded.size.w;

        let result = if want_wrap {
            let target = available_w.max(unbounded.intrinsic_min);
            let target_q = quantize_wrap_target(target);
            text.shape_wrap(wid, src, font_size_px, target, target_q)
        } else {
            unbounded
        };

        self.result.set_text_shape(
            node,
            ShapedText {
                measured: result.size,
                key: result.key,
            },
        );
        result.size
    }
}
