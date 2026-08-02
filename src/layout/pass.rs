//! [`LayoutPass`] — the borrows one layer's measure/arrange walk holds,
//! and the recursion that rides them.
//!
//! Every driver used to take the same five or six threaded parameters
//! (`engine, tree, node, …, interned_text, out`) and reach into engine
//! state by path (`engine.scratch.grid.depth_stack`). Both are this
//! type's job now: the invariant borrows live here once, and the scratch
//! each driver owns is reached by name. A driver's signature is down to
//! the pass plus what actually varies per node.
//!
//! The intrinsic query deliberately does **not** live here. It is a pure
//! function of a subtree and must not be able to write the frame's text
//! shapes, which staying on [`LayoutEngine`] — where no `LayerLayout` is
//! in reach — enforces by construction. [`LayoutPass::intrinsic`] and
//! [`LayoutPass::intrinsic_range`] are one-line forwarders so driver call
//! sites stay short without widening what the query can touch.

use crate::layout::LayerLayout;
use crate::layout::axis::Axis;
use crate::layout::cache::quantize_available;
use crate::layout::engine::{
    LayoutEngine, NO_ARRANGE_SRC, resolve_sizing, restore_after_cache_hit,
};
use crate::layout::grid::{GridContext, GridHugStore};
use crate::layout::intrinsic::{IntrinsicRange, LenReq};
use crate::layout::probe::PhaseSpan;
use crate::layout::stack::StackScratch;
use crate::layout::support::{TextShapeInput, leaf_text_shapes};
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::wrapstack::WrapScratch;
use crate::layout::{canvas, grid, scroll, scrollbars, stack, wrapstack, zstack};
use crate::primitives::interned_str::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::scene::node::columns::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::record::NodeId;
use crate::text::system::TextRunSlot;
use glam::Vec2;

/// One layer's measure/arrange walk: the engine it mutates, the tree it
/// reads, the text arena its shapes resolve against, and the column
/// block it fills.
///
/// Constructed per layer by [`LayoutEngine::run`] and threaded through
/// every driver by `&mut`. `tree` and `interned_text` are `pub(super)`
/// because they are shared references to frozen input — there is nothing
/// to encapsulate. `engine` and `out` are private: the accessors below
/// are the whole surface a driver is meant to reach, which is what keeps
/// a driver from growing a dependency on some other driver's scratch.
#[derive(Debug)]
pub(crate) struct LayoutPass<'a> {
    engine: &'a mut LayoutEngine,
    pub(super) tree: &'a Tree,
    pub(super) interned_text: &'a InternedText<'a>,
    out: &'a mut LayerLayout,
}

impl<'a> LayoutPass<'a> {
    pub(super) fn new(
        engine: &'a mut LayoutEngine,
        tree: &'a Tree,
        interned_text: &'a InternedText<'a>,
        out: &'a mut LayerLayout,
    ) -> Self {
        Self {
            engine,
            tree,
            interned_text,
            out,
        }
    }
}

/// Scratch and column access. One method per thing a driver legitimately
/// touches; nothing hands back `&mut LayoutEngine` or `&mut LayerLayout`
/// whole.
impl LayoutPass<'_> {
    /// This node's measured size, as `measure` left it. Arrange reads it
    /// for every child it places.
    #[inline]
    pub(super) fn desired(&self, node: NodeId) -> Size {
        self.engine.scratch.desired[node.idx()]
    }

    /// Grid's per-depth track scratch and durable hug pool. Handed back
    /// whole because `grid` disjoint-borrows the two halves in one
    /// expression.
    #[inline]
    pub(super) fn grid_mut(&mut self) -> &mut GridContext {
        &mut self.engine.scratch.grid
    }

    /// The hug pool alone, for the sites that don't also need the depth
    /// stack.
    #[inline]
    pub(super) fn grid_hugs_mut(&mut self) -> &mut GridHugStore {
        &mut self.engine.scratch.grid.hugs
    }

    /// Stack's flat Fill-entry pool, shared across nesting depths.
    #[inline]
    pub(super) fn stack_scratch_mut(&mut self) -> &mut StackScratch {
        &mut self.engine.scratch.stack_fill
    }

    /// WrapStack's flat per-depth line buffer.
    #[inline]
    pub(super) fn wrap_scratch_mut(&mut self) -> &mut WrapScratch {
        &mut self.engine.scratch.wrap
    }

    /// Measured content extent of a scroll viewport, written by
    /// `scroll::measure` and read by `scrollbars::arrange` to size its
    /// thumbs — the one column one driver writes for another.
    #[inline]
    pub(super) fn scroll_content(&self, node: NodeId) -> Size {
        self.out.scroll_content[node.idx()]
    }

    #[inline]
    pub(super) fn set_scroll_content(&mut self, node: NodeId, content: Size) {
        self.out.scroll_content[node.idx()] = content;
    }

    /// Anchor this node and every descendant at a zero-size rect —
    /// what a collapsed subtree gets. Walks the contiguous pre-order
    /// span directly; no recursion, no child cursors.
    #[inline]
    pub(super) fn zero_subtree(&mut self, node: NodeId, anchor: Vec2) {
        let start = node.idx();
        let end = self.tree.subtree_end_of(start) as usize;
        self.out.rect[start..end].fill(Rect {
            min: anchor,
            size: Size::ZERO,
        });
    }

    /// This node's arranged rect, as `arrange` left it. Read by the
    /// container-text pass, which shapes against the width arrange
    /// committed.
    #[inline]
    pub(super) fn rect(&self, node: NodeId) -> Rect {
        self.out.rect[node.idx()]
    }

    /// Fold a closed measure span into this run's probe. The spans are
    /// opened around the root walk, which happens inside the pass, so
    /// they close through it too.
    #[inline]
    pub(super) fn note_measure(&mut self, span: PhaseSpan) {
        self.engine.scratch.probe.add_measure(span);
    }

    #[inline]
    pub(super) fn note_arrange(&mut self, span: PhaseSpan) {
        self.engine.scratch.probe.add_arrange(span);
    }

    /// Outer intrinsic on `axis` under content-sizing `req`. Forwards to
    /// [`LayoutEngine::intrinsic`] — see this module's doc for why the
    /// query itself stays off the pass.
    #[inline]
    pub(super) fn intrinsic(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
        self.engine
            .intrinsic(self.tree, node, axis, req, self.interned_text)
    }

    /// Paired min/max-content query — [`LayoutEngine::intrinsic_range`].
    #[inline]
    pub(super) fn intrinsic_range(&mut self, node: NodeId, axis: Axis) -> IntrinsicRange {
        self.engine
            .intrinsic_range(self.tree, node, axis, self.interned_text)
    }
}

/// The recursion itself.
impl LayoutPass<'_> {
    /// Bottom-up measure dispatcher. Drivers call back here to recurse.
    /// Stores the resolved size for each visited node, which `arrange`
    /// then reads through [`Self::desired`].
    pub(super) fn measure(&mut self, node: NodeId, available: Size) -> Size {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        let available_q = quantize_available(available);
        self.engine.scratch.available_q[node.idx()] = available_q;
        if layout.meta.visibility().is_collapsed() {
            self.engine.scratch.desired[node.idx()] = Size::ZERO;
            return Size::ZERO;
        }

        // Phase-2 measure-cache short-circuit: any non-leaf node. Same
        // `WidgetId`, same rolled subtree hash, same quantized
        // `available` → restore the *whole subtree*'s `desired` and
        // text shapes from last frame's snapshot and skip recursion
        // entirely. The subtree-hash rollup guarantees structural and
        // authoring equivalence; `available_q` guards against parent
        // resize since outer-leaf measure is `available`-dependent
        // for `Hug` / `Fill` axes.
        if LayoutMode::from(layout.meta) != LayoutMode::Leaf {
            let cache_wid = tree.records.widget_id()[node.idx()];
            let cache_hash = tree.rollups.subtree[node.idx()];
            if let Some(hit) = self
                .engine
                .cache
                .try_lookup(cache_wid, cache_hash, available_q)
            {
                self.engine.scratch.probe.cache_hit(cache_wid);
                let curr_start = node.idx();
                let curr_end = curr_start + hit.desired.len();
                // Subtree hash includes child count + per-child rollups,
                // so a length mismatch here would mean the rollup is broken.
                debug_assert_eq!(curr_end, tree.subtree_end_of(curr_start) as usize);
                self.engine.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
                self.engine.scratch.arrange_src[curr_start] = hit.nodes_base;
                restore_after_cache_hit(
                    &mut self.engine.scratch,
                    tree,
                    curr_start..curr_end,
                    &hit,
                    self.out,
                    self.engine.cache_rebuild,
                );
                return hit.root;
            }
        }

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
        // Per-axis gate: `Sizing::fixed` ignores `intrinsic_min` in
        // both `resolve_axis_size` (Fixed branch returns `v` verbatim)
        // and the `dispatch_avail.max(intrinsic_min)` floor below
        // (Fixed reads neither side). Skip the query on Fixed axes so
        // a Fixed leaf doesn't trigger a subtree intrinsic walk every
        // frame.
        let intrinsic_min = Size::new(
            if layout.size.w().fixed_value().is_some() {
                0.0
            } else {
                self.intrinsic(node, Axis::X, LenReq::MinContent)
            },
            if layout.size.h().fixed_value().is_some() {
                0.0
            } else {
                self.intrinsic(node, Axis::Y, LenReq::MinContent)
            },
        );

        // Derive `inner_avail`, dispatch to the driver, fold its raw
        // content into a margin-inclusive `desired`. `resolve_sizing`
        // contains the rationale for each step (intrinsic_min floor,
        // outer clamp to `[min, max]`, single-dispatch monotonicity).
        let desired = resolve_sizing(
            layout,
            available,
            intrinsic_min,
            min_size,
            max_size,
            |inner_avail| self.measure_dispatch(node, layout, inner_avail),
        );

        self.engine.scratch.desired[node.idx()] = desired;

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
    /// `grid`) is a free module exporting three fns (visible at least to
    /// the rest of `layout`), matched into here and into
    /// [`Self::arrange`] / `intrinsic::compute`:
    ///
    /// - `measure(pass, node, [variant_payload,] inner_avail) -> Size`
    ///   — bottom-up. Recurses into children via `pass.measure(...)`.
    ///   Returns the driver's content size (pre-padding/margin/clamp;
    ///   the caller in [`Self::measure`] folds those in).
    /// - `arrange(pass, node, [variant_payload,] inner)`
    ///   — top-down. Assigns each child a final rect and recurses via
    ///   `pass.arrange(...)`.
    /// - `intrinsic(engine, tree, node, [variant_payload,] axis, req, interned_text) -> f32`
    ///   — pure on-demand query, and the one that takes no pass: it must
    ///   not reach the frame's text shapes. Used by `grid::measure`
    ///   Phase-1 column resolution and `stack::measure`'s Fill
    ///   min-content floor.
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
    fn measure_dispatch(&mut self, node: NodeId, layout: LayoutCore, inner_avail: Size) -> Size {
        match LayoutMode::from(layout.meta) {
            LayoutMode::Leaf => {
                let (tree, interned_text) = (self.tree, self.interned_text);
                let runs = leaf_text_shapes(tree, interned_text, node);
                self.shape_text_runs(node, inner_avail.w, runs)
            }
            LayoutMode::HStack => stack::measure(self, node, inner_avail, Axis::X),
            LayoutMode::VStack => stack::measure(self, node, inner_avail, Axis::Y),
            LayoutMode::WrapHStack => wrapstack::measure(self, node, inner_avail, Axis::X),
            LayoutMode::WrapVStack => wrapstack::measure(self, node, inner_avail, Axis::Y),
            LayoutMode::ZStack => zstack::measure(self, node, inner_avail),
            LayoutMode::Canvas => canvas::measure(self, node, inner_avail),
            LayoutMode::Grid(grid_def_id) => grid::measure(self, node, grid_def_id, inner_avail),
            // Scroll viewport. INF-axis measure of children.
            LayoutMode::Scrollbars(_) => scrollbars::measure(self, node, inner_avail),
            LayoutMode::Scroll(scroll_spec) => {
                scroll::measure(self, node, inner_avail, scroll_spec)
            }
        }
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in the
    /// active layer's `Layout`.
    pub(super) fn arrange(&mut self, node: NodeId, slot: Rect) {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        if layout.meta.visibility().is_collapsed() {
            self.zero_subtree(node, slot.min);
            return;
        }
        let rendered = slot.deflated_by(layout.margin);
        let mode = LayoutMode::from(layout.meta);
        // Replay's precondition is that arrange is a pure function of the
        // slot (see `replay_arranged`). `Scrollbars` is the one driver
        // that isn't: it reads a *sibling's* measured `scroll_content`, so
        // its bars must be re-placed even when its own subtree hash and
        // slot are unchanged — otherwise a scroll whose content stopped
        // overflowing keeps painting the old thumb.
        // Replay is only sound for a driver whose arrange reads nothing
        // but its own subtree and this slot; the mode declares that.
        if mode.arrange_depends_only_on_slot() && self.replay_arranged(node, rendered) {
            return;
        }
        self.out.rect[node.idx()] = rendered;
        let inner = rendered.deflated_by(layout.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, node, inner, Axis::X),
            LayoutMode::VStack => stack::arrange(self, node, inner, Axis::Y),
            LayoutMode::WrapHStack => wrapstack::arrange(self, node, inner, Axis::X),
            LayoutMode::WrapVStack => wrapstack::arrange(self, node, inner, Axis::Y),
            LayoutMode::ZStack => zstack::arrange(self, node, inner),
            LayoutMode::Canvas => canvas::arrange(self, node, inner),
            LayoutMode::Grid(grid_def_id) => grid::arrange(self, node, inner, grid_def_id),
            LayoutMode::Scroll(scroll_spec) => scroll::arrange(self, node, inner, scroll_spec),
            LayoutMode::Scrollbars(id) => {
                let def = tree.scrollbar_defs[usize::from(id)];
                scrollbars::arrange(self, node, inner, def);
            }
        }
    }

    /// Replay a measure-cache-hit subtree's arranged rects instead of
    /// re-running the drivers over it. Returns `false` when the subtree
    /// must be arranged normally.
    ///
    /// Sound because arrange's **only** output is `out.rect` — every
    /// driver's `arrange` writes rects and recurses, and nothing else
    /// (`scroll::arrange` merely delegates to stack/zstack; container text
    /// shapes later in [`LayoutEngine::run`], off this path). So for a
    /// subtree whose authoring and `desired` are both known identical to
    /// the snapshot — which is exactly what a measure hit proves — arrange
    /// is a pure function of the slot it is handed.
    ///
    /// That reasoning covers every driver whose arrange stays inside its
    /// own subtree, which is not all of them; the caller gates on
    /// [`LayoutMode::arrange_depends_only_on_slot`] so a driver reading
    /// outside itself never reaches here.
    ///
    /// Two of the three slot outcomes replay:
    ///
    /// - **Unchanged** rendered rect: a straight `copy_from_slice`.
    /// - **Translated** (same size, moved origin — a sibling above grew,
    ///   so everything below shifts): one add per node over a contiguous
    ///   `Rect` slice, which is what the drivers would have spent a full
    ///   dispatch to arrive at.
    /// - **Resized**: bails to the normal path. A different size
    ///   redistributes `Fill` children, so nothing below is reusable.
    ///
    /// Indexing is safe by construction: the destination range comes from
    /// the *current* tree while the source is keyed by `WidgetId`, so a
    /// subtree that moved in pre-order still replays into its new slot.
    /// Collapsed descendants ride along — [`Self::zero_subtree`] anchors
    /// them at their parent's slot origin, which translates with
    /// everything else.
    #[inline]
    fn replay_arranged(&mut self, node: NodeId, rendered: Rect) -> bool {
        let base = self.engine.scratch.arrange_src[node.idx()];
        if base == NO_ARRANGE_SRC {
            return false;
        }
        let start = node.idx();
        let end = self.tree.subtree_end_of(start) as usize;
        let base = base as usize;
        let src = &self.engine.cache.previous.nodes.rect[base..base + (end - start)];
        if src[0].size != rendered.size {
            return false;
        }
        let delta = rendered.min - src[0].min;
        let dst = &mut self.out.rect[start..end];
        if delta == Vec2::ZERO {
            self.engine.scratch.probe.arrange_copied();
            dst.copy_from_slice(src);
        } else {
            self.engine.scratch.probe.arrange_translated();
            for (d, s) in dst.iter_mut().zip(src) {
                *d = Rect {
                    min: s.min + delta,
                    size: s.size,
                };
            }
        }
        true
    }

    /// Shape every text run `runs` yields for `node`, append them to the
    /// frame's flat buffer, and stamp the covering span. Returns the
    /// largest run's content size — a leaf's text contribution.
    pub(super) fn shape_text_runs<'t>(
        &mut self,
        node: NodeId,
        available_w: f32,
        runs: impl Iterator<Item = TextShapeInput<'t>>,
    ) -> Size {
        let span_start = self.out.text_shapes.len() as u32;
        let mut s = Size::ZERO;
        for ts in runs {
            let m = self.shape_text(node, &ts, available_w);
            s = s.max(m);
        }
        let span_len = self.out.text_shapes.len() as u32 - span_start;
        self.out.text_spans[node.idx()] = Span {
            start: span_start,
            len: span_len,
        };
        s
    }

    fn shape_text(&mut self, node: NodeId, ts: &TextShapeInput<'_>, available_w: f32) -> Size {
        let wid = self.tree.records.widget_id()[node.idx()];
        let slot = TextRunSlot {
            widget_id: wid,
            ordinal: ts.ordinal,
        };

        let shaped = self.engine.text.measure(
            slot,
            ts.shape_request(),
            ts.wrap,
            ts.halign,
            available_w.is_finite().then_some(available_w),
        );

        self.out.text_shapes.push(shaped);
        ts.wrap.content_size(shaped.measured)
    }
}
