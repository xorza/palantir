//! [`LayoutPass`] — the borrows one layer's measure/arrange walk holds,
//! and the recursion that rides them.
//!
//! The invariant borrows live here once, so a driver takes the pass plus
//! what actually varies per node — not the five or six parameters
//! (`engine, tree, node, …, interned_text, out`) each would otherwise
//! thread identically. The scratch a driver owns is reached by name
//! through the accessors below rather than by path into engine state
//! (`engine.scratch.grid.depth_stack`), which is what keeps one driver
//! from growing a dependency on another's.
//!
//! The intrinsic query deliberately does **not** live here. It is a pure
//! function of a subtree and must not be able to write the frame's text
//! shapes, which staying on [`LayoutEngine`] — where no `LayerLayout` is
//! in reach — enforces by construction. [`LayoutPass::intrinsic`] and
//! [`LayoutPass::intrinsic_range`] are one-line forwarders so driver call
//! sites stay short without widening what the query can touch.

use crate::layout::LayerLayout;
use crate::layout::axis::Axis;
use crate::layout::axis_ctx::AxisCtx;
use crate::layout::cache::quantize_available;
use crate::layout::counters::PhaseSpan;
use crate::layout::driver::{DriverOp, LayoutDriver, ReplayOp};
use crate::layout::engine::LayoutEngine;
use crate::layout::grid::grid_context::GridContext;
use crate::layout::grid::grid_track_store::GridTrackStore;
use crate::layout::intrinsic::{IntrinsicRange, LenReq};
use crate::layout::layout_scratch::NO_ARRANGE_SRC;
use crate::layout::stack::StackScratch;
use crate::layout::text_shape_input::TextShapeInput;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::wrapstack::WrapScratch;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
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
    /// Measure children of a per-axis-hug panel (ZStack / Canvas). Per
    /// active child, calls `layout.measure` against the per-axis-hug
    /// `child_avail`, then folds the child's contribution (size + offset
    /// from `contrib`) into a per-axis max. Drivers differ only in
    /// whether they add a positional offset.
    pub(crate) fn measure_per_axis_hug(
        &mut self,
        node: NodeId,
        inner_avail: Size,
        mut contrib: impl FnMut(&Tree, NodeId, Size) -> Size,
    ) -> Size {
        let tree = self.tree;
        let node_layout = tree.records.layout()[node.idx()];
        // Per-axis-hug availability: a `Hug` axis passes `INF` so the child
        // reports its natural size; a bounded axis passes the committed inner
        // extent. `INF` here is *height-given-width* via measure, not an
        // intrinsic-replaceable sentinel — replacing it with
        // `intrinsic(MaxContent)` looks equivalent for leaves but is wrong for
        // nested containers whose main-axis size depends on cross-axis (Grid
        // with wrapping cells, etc.): intrinsic queries the unbounded shape,
        // while INF-measure runs the child's full layout under the committed cross.
        let child_avail = Size::new(
            if node_layout.size.w().is_hug() {
                f32::INFINITY
            } else {
                inner_avail.w
            },
            if node_layout.size.h().is_hug() {
                f32::INFINITY
            } else {
                inner_avail.h
            },
        );
        let mut max_w = 0.0f32;
        let mut max_h = 0.0f32;
        for c in tree.active_children(node) {
            let d = self.measure(c, child_avail);
            let cont = contrib(tree, c, d);
            max_w = max_w.max(cont.w);
            max_h = max_h.max(cont.h);
        }
        Size::new(max_w, max_h)
    }

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

    /// The durable per-grid track store alone, for the sites that don't
    /// also need the depth stack.
    #[inline]
    pub(super) fn grid_track_state_mut(&mut self) -> &mut GridTrackStore {
        &mut self.engine.scratch.grid.track_state
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
    /// `Scroll::measure` and read by `Scrollbars::arrange` to size its
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
        let end = self.tree.subtree_end_of(start);
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
        self.engine.scratch.counters.add_measure(span);
    }

    #[inline]
    pub(super) fn note_arrange(&mut self, span: PhaseSpan) {
        self.engine.scratch.counters.add_arrange(span);
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
                self.engine.scratch.counters.cache_hit(cache_wid);
                let curr_start = node.idx();
                let curr_end = curr_start + hit.desired.len();
                // Subtree hash includes child count + per-child rollups,
                // so a length mismatch here would mean the rollup is broken.
                debug_assert_eq!(curr_end, tree.subtree_end_of(curr_start));
                self.engine.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
                self.engine.scratch.arrange_src[curr_start] = hit.nodes_base;
                self.engine.scratch.restore_after_cache_hit(
                    tree,
                    curr_start..curr_end,
                    &hit,
                    self.out,
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
        // both `AxisCtx::resolve` (Fixed branch returns `v` verbatim)
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
        // content into a margin-inclusive `desired`. `AxisCtx::resolve_node`
        // contains the rationale for each step (intrinsic_min floor,
        // outer clamp to `[min, max]`, single-dispatch monotonicity).
        let desired = AxisCtx::resolve_node(
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
    /// `AxisCtx::resolve_node` for why no re-measure is needed when a Fill
    /// axis grows past `available`); the caller folds content into a
    /// margin-inclusive `desired` via `AxisCtx::resolve`.
    ///
    /// The contract every driver answers to is
    /// [`LayoutDriver`](crate::layout::driver::LayoutDriver); the match
    /// that picks one is `DriverOp::dispatch`, shared with
    /// [`Self::arrange`] and `intrinsic::content_intrinsic`.
    fn measure_dispatch(&mut self, node: NodeId, layout: LayoutCore, inner_avail: Size) -> Size {
        MeasureOp {
            pass: self,
            node,
            inner_avail,
        }
        .dispatch(LayoutMode::from(layout.meta))
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
        if ReplayOp.dispatch(mode) && self.replay_arranged(node, rendered) {
            return;
        }
        self.out.rect[node.idx()] = rendered;
        let inner = rendered.deflated_by(layout.padding);

        ArrangeOp {
            pass: self,
            node,
            inner,
        }
        .dispatch(mode);
    }

    /// Replay a measure-cache-hit subtree's arranged rects instead of
    /// re-running the drivers over it. Returns `false` when the subtree
    /// must be arranged normally.
    ///
    /// Sound because arrange's **only** output is `out.rect` — every
    /// driver's `arrange` writes rects and recurses, and nothing else
    /// (`Scroll::arrange` merely delegates to stack/zstack; container text
    /// shapes later in [`LayoutEngine::run`], off this path). So for a
    /// subtree whose authoring and `desired` are both known identical to
    /// the snapshot — which is exactly what a measure hit proves — arrange
    /// is a pure function of the slot it is handed.
    ///
    /// That reasoning covers every driver whose arrange stays inside its
    /// own subtree, which is not all of them; the caller gates on
    /// [`LayoutDriver::ARRANGE_DEPENDS_ONLY_ON_SLOT`] so a driver reading
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
        let end = self.tree.subtree_end_of(start);
        let base = base as usize;
        let src = &self.engine.cache.previous.nodes.rect[base..base + (end - start)];
        if src[0].size != rendered.size {
            return false;
        }
        let delta = rendered.min - src[0].min;
        let dst = &mut self.out.rect[start..end];
        if delta == Vec2::ZERO {
            self.engine.scratch.counters.arrange_copied();
            dst.copy_from_slice(src);
        } else {
            self.engine.scratch.counters.arrange_translated();
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

#[derive(Debug)]
struct MeasureOp<'op, 'pass> {
    pass: &'op mut LayoutPass<'pass>,
    node: NodeId,
    inner_avail: Size,
}

impl DriverOp for MeasureOp<'_, '_> {
    type Output = Size;

    fn run<D: LayoutDriver>(self, payload: D::Payload) -> Size {
        D::measure(self.pass, self.node, payload, self.inner_avail)
    }

    /// A leaf's content size is its shaped text, wrapped against the
    /// width it was offered.
    fn leaf(self) -> Size {
        let Self {
            pass,
            node,
            inner_avail,
        } = self;
        let (tree, interned_text) = (pass.tree, pass.interned_text);
        let runs = TextShapeInput::on_leaf(tree, interned_text, node);
        pass.shape_text_runs(node, inner_avail.w, runs)
    }
}

#[derive(Debug)]
struct ArrangeOp<'op, 'pass> {
    pass: &'op mut LayoutPass<'pass>,
    node: NodeId,
    inner: Rect,
}

impl DriverOp for ArrangeOp<'_, '_> {
    type Output = ();

    fn run<D: LayoutDriver>(self, payload: D::Payload) {
        D::arrange(self.pass, self.node, payload, self.inner);
    }

    /// A leaf has no children to place; its own rect is already stored.
    fn leaf(self) {}
}
