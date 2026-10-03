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

use crate::layout::axis::Axis;
use crate::layout::axis_placement::Placed;
use crate::layout::axis_slot::AxisSlot;
use crate::layout::cache::MeasureCache;
use crate::layout::counters::PhaseSpan;
use crate::layout::driver::{DriverOp, LayoutDriver, ReplayOp};
use crate::layout::engine::LayoutEngine;
use crate::layout::grid::grid_context::GridContext;
use crate::layout::grid::grid_track_store::GridTrackStore;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::layout_scratch::NO_ARRANGE_SRC;
use crate::layout::measured::Measured;
use crate::layout::stack::stack_scratch::StackScratch;
use crate::layout::text_shape_input::TextShapeInput;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::wrapstack::WrapScratch;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::system::{RunMeasure, TextRunSlot};
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
    /// Min-content intrinsic on one axis — the smallest this node can
    /// shrink to without breaking a rigid descendant (Fixed widget,
    /// explicit `min_size`, longest unbreakable word).
    ///
    /// Fed into `AxisSlot::resolve` as the lower bound under flex
    /// semantics: Hug/Fill clamp down to `available` but never below
    /// this. Cached per (node, axis, slot), so repeat queries during
    /// the same `run` are O(1).
    ///
    /// **Zero on a Fixed axis.** `Sizing::fixed` ignores `intrinsic_min`
    /// in both `AxisSlot::resolve` (the Fixed branch returns `v`
    /// verbatim) and the `dispatch_avail.max(intrinsic_min)` floor
    /// (Fixed reads neither side). Skipping the query there is what
    /// keeps a Fixed leaf from triggering a subtree intrinsic walk every
    /// frame.
    fn intrinsic_min(&mut self, node: NodeId, layout: LayoutCore, axis: Axis) -> f32 {
        if axis.main_sizing(layout.size).fixed_value().is_some() {
            return 0.0;
        }
        self.intrinsic(node, axis, LenReq::MinContent)
    }

    /// Measure the children of a per-axis-hug panel (ZStack / Canvas) and
    /// return the extent that covers them.
    ///
    /// `offset` is where the panel places a child inside its own inner
    /// rect — always zero for a ZStack, the declared position for a
    /// Canvas. Every axis offers the child the room left *past* the
    /// offset: what `arrange` will hand it on a bounded axis, and on a
    /// Hug axis the most the panel can grow to, since a Hug panel resolves
    /// to `min(content, available)`. Offering more makes a wrapping child
    /// report a height for a width it will not get — `Stack` offers its
    /// cross axis the same finite room. An unbounded parent still offers
    /// `INFINITY`, and the room past it stays unbounded.
    ///
    /// The extent reported differs per axis: a bounded axis reports the
    /// child's own extent, and a Hug axis folds the offset back in,
    /// because a panel that hugs has to cover where it put things.
    pub(super) fn measure_per_axis_hug(
        &mut self,
        node: NodeId,
        inner_avail: Size,
        mut offset: impl FnMut(&Tree, NodeId) -> Vec2,
    ) -> Measured {
        let tree = self.tree;
        let hug = tree.records.layout()[node.idx()].size.hug_mask();
        // The child holds while the room past its position stays past its
        // range, so the range moves out by the position too.
        let room_from = |from: f32, offset: f32| if from > 0.0 { from + offset } else { 0.0 };
        let mut max = Measured::ZERO;
        for c in tree.active_children(node) {
            let at = offset(tree, c);
            // Both kinds of axis offer the room past the child's position;
            // a hug axis then grows to cover where the child was put, and a
            // bounded one reports the child's extent alone. The floor
            // follows the extent.
            let child = self.measure(c, inner_avail.room_past(at));
            let covered = |d: Size| Size::new(at.x + d.w, at.y + d.h).select(hug, d);
            max = Measured {
                size: max.size.max(covered(child.size)),
                floor: max.floor.max(covered(child.floor)),
                stable_from: max.stable_from.max(Size::new(
                    room_from(child.stable_from.w, at.x),
                    room_from(child.stable_from.h, at.y),
                )),
            };
        }
        max
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

    /// What this node measured to, as a parent places it: [`Self::desired`]
    /// and the floor it gives way to no further — see [`Placed::of`].
    #[inline]
    pub(super) fn placed(&self, node: NodeId) -> Placed {
        Placed::of(self.desired(node), self.engine.scratch.floor[node.idx()])
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

    /// Stack's flat Fill and Hug pools, shared across nesting depths.
    #[inline]
    pub(super) fn stack_scratch_mut(&mut self) -> &mut StackScratch {
        &mut self.engine.scratch.stack
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
    /// then reads through [`Self::desired`], and its floor, which the
    /// cache keeps for the subtree's next hit.
    pub(super) fn measure(&mut self, node: NodeId, available: Size) -> Measured {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        let available_q = MeasureCache::available_key(available);
        self.engine.scratch.available_q[node.idx()] = available_q;
        // `is_collapsed`, not `!is_visible`: a `Hidden` node keeps its
        // slot, so its extent still has to be measured — a text leaf
        // shapes its run to find one, even though nothing paints it.
        // That is the opposite of the container-text pass in
        // `LayoutEngine::run`, whose runs are paint-only.
        if layout.meta.visibility().is_collapsed() {
            self.engine.scratch.desired[node.idx()] = Size::ZERO;
            self.engine.scratch.floor[node.idx()] = Size::ZERO;
            self.engine.scratch.stable_from[node.idx()] = Size::ZERO;
            return Measured::ZERO;
        }

        // Phase-2 measure-cache short-circuit: any non-leaf node. Same
        // `WidgetId`, same rolled subtree hash, and an `available` the
        // last measure holds under → restore the *whole subtree*'s
        // `desired` and text shapes from last frame's snapshot and skip
        // recursion entirely. The subtree-hash rollup guarantees
        // structural and authoring equivalence; the offer check guards
        // against a parent resize, since a `Hug` or `Fill` axis can read
        // its offer.
        if LayoutMode::from(layout.meta) != LayoutMode::Leaf {
            let cache_wid = tree.records.widget_id()[node.idx()];
            let cache_hash = tree.rollups.layout_subtree[node.idx()];
            if let Some(hit) =
                self.engine
                    .cache
                    .try_lookup(cache_wid, cache_hash, available, available_q)
            {
                self.engine.scratch.counters.cache_hit(cache_wid);
                let curr_start = node.idx();
                let curr_end = curr_start + hit.desired.len();
                // Subtree hash includes child count + per-child rollups,
                // so a length mismatch here would mean the rollup is broken.
                debug_assert_eq!(curr_end, tree.subtree_end_of(curr_start));
                self.engine.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
                // Every node of the subtree, not only its root: each one's
                // desired and authoring is proven alike, so a descendant
                // arranged at its cached size replays even where the root,
                // arranged at a new size, has to dispatch.
                for (src, offset) in self.engine.scratch.arrange_src[curr_start..curr_end]
                    .iter_mut()
                    .zip(0..)
                {
                    *src = hit.nodes_base + offset;
                }
                self.engine.scratch.restore_after_cache_hit(
                    tree,
                    curr_start..curr_end,
                    &hit,
                    self.out,
                );
                // The root is recorded under this offer, which its measure
                // holds at, rather than the one restored with its subtree:
                // a root slot keyed on a stale offer fails
                // `MeasureCache::matches_forest` and rebuilds every frame.
                self.engine.scratch.available_q[curr_start] = available_q;
                return hit.root;
            }
        }

        let bounds = tree.bounds(node);
        let (min_size, max_size) = (bounds.min_size, bounds.max_size);

        let intrinsic_min = Size::new(
            self.intrinsic_min(node, layout, Axis::X),
            self.intrinsic_min(node, layout, Axis::Y),
        );

        // Derive `inner_avail`, dispatch to the driver, fold its raw
        // content and floor into a margin-inclusive `desired` and floor.
        // `AxisSlot::resolve_node` contains the rationale for each step
        // (the floors, outer clamp to `[min, max]`, single-dispatch
        // monotonicity).
        let measured = AxisSlot::resolve_node(
            layout,
            available,
            intrinsic_min,
            min_size,
            max_size,
            |inner_avail| self.measure_dispatch(node, layout, inner_avail),
        );

        self.engine.scratch.desired[node.idx()] = measured.size;
        self.engine.scratch.floor[node.idx()] = measured.floor;
        self.engine.scratch.stable_from[node.idx()] = measured.stable_from;

        measured
    }

    /// Dispatch one driver measure for `node` against the
    /// already-derived `inner_avail`; returns the driver's raw content
    /// size and floor. Called exactly once per `measure` (single dispatch — see
    /// `AxisSlot::resolve_node` for why no re-measure is needed when a Fill
    /// axis grows past `available`); the caller folds content into a
    /// margin-inclusive `desired` via `AxisSlot::resolve`.
    ///
    /// The contract every driver answers to is [`LayoutDriver`]; the
    /// match that picks one is `DriverOp::dispatch`, shared with
    /// [`Self::arrange`] and `IntrinsicQuery::walk`.
    fn measure_dispatch(
        &mut self,
        node: NodeId,
        layout: LayoutCore,
        inner_avail: Size,
    ) -> Measured {
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
    ///
    /// **A collapsed subtree is zeroed here**, at the slot its parent
    /// placed it in, so a driver that places every child alike hands
    /// collapsed ones the same slot as the rest and says nothing about
    /// them. Only a driver whose *own* bookkeeping a collapsed child must
    /// not advance — the two stacks' main-axis cursor — tests for it, and
    /// what those pass is a different anchor rather than the same one
    /// twice. The position of a zero-size rect is not observable beyond
    /// being stable, so the two anchors are equally good answers.
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
        let inner = layout.inner_rect(rendered);

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
        let src = self.engine.cache.arranged_rects(base, end - start);
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
    /// frame's flat buffer, and stamp the covering span. Returns the runs'
    /// contribution to a leaf: the largest run's content size, its height
    /// as the floor — the lines the text wrapped to at this width do not
    /// shrink — and the widths every run answers the same under. No
    /// offer of height reaches text.
    pub(super) fn shape_text_runs<'t>(
        &mut self,
        node: NodeId,
        available_w: f32,
        runs: impl Iterator<Item = TextShapeInput<'t>>,
    ) -> Measured {
        let wid = self.tree.records.widget_id()[node.idx()];
        let span_start = self.out.text_shapes.len() as u32;
        let mut size = Size::ZERO;
        let mut stable_from_w = 0.0f32;
        for ts in runs {
            let run = self.shape_text(wid, &ts, available_w);
            size = size.max(ts.wrap.content_size(run.shaped.extent.size));
            stable_from_w = stable_from_w.max(run.stable_from_w);
        }
        let span_len = self.out.text_shapes.len() as u32 - span_start;
        self.out.text_spans[node.idx()] = Span {
            start: span_start,
            len: span_len,
        };
        // The count this node just recorded is the only point that knows
        // it, so the reuse rows above it are dropped here. A node that
        // filled every ordinal has none above it, and is also the one
        // `count` cannot express.
        if let Ok(count) = u16::try_from(span_len) {
            self.engine.text.trim_rows(wid, count);
        }
        Measured {
            size,
            floor: Size::new(0.0, size.h),
            stable_from: Size::new(stable_from_w, 0.0),
        }
    }

    fn shape_text(
        &mut self,
        wid: WidgetId,
        ts: &TextShapeInput<'_>,
        available_w: f32,
    ) -> RunMeasure {
        let slot = TextRunSlot {
            widget_id: wid,
            ordinal: ts.ordinal,
        };

        let run = self.engine.text.measure(
            slot,
            ts.shape_request(),
            ts.wrap,
            ts.halign,
            available_w.is_finite().then_some(available_w),
        );
        self.out.text_shapes.push(run.shaped);
        run
    }
}

#[derive(Debug)]
struct MeasureOp<'op, 'pass> {
    pass: &'op mut LayoutPass<'pass>,
    node: NodeId,
    inner_avail: Size,
}

impl DriverOp for MeasureOp<'_, '_> {
    type Output = Measured;

    fn run<D: LayoutDriver>(self, payload: D::Payload) -> Measured {
        D::measure(self.pass, self.node, payload, self.inner_avail)
    }

    /// A leaf's content is its shaped text, wrapped against the width it
    /// was offered. Its floor across is the intrinsic minimum the caller
    /// already holds.
    fn leaf(self) -> Measured {
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
