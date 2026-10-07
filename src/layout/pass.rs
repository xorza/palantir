//! [`LayoutPass`]: the borrows one layer's measure/arrange walk holds, and the
//! recursion that rides them. Driver scratch is reached through the accessors
//! below, so one driver can't grow a dependency on another's.
//!
//! The intrinsic query deliberately does not live here: it is pure and must not
//! write the frame's text shapes, which staying on [`LayoutEngine`] (no
//! `LayerLayout` in reach) enforces by construction. [`LayoutPass::intrinsic`] and
//! [`LayoutPass::intrinsic_range`] just forward.

use crate::common::span::Span;
use crate::layout::axis_placement::Placed;
use crate::layout::axis_slot::AxisSlot;
use crate::layout::cache::{Arranged, MeasureCache};
use crate::layout::counters::PhaseSpan;
use crate::layout::drivers::grid::grid_context::GridContext;
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;
use crate::layout::drivers::stack::stack_scratch::StackScratch;
use crate::layout::drivers::wrapstack::WrapScratch;
use crate::layout::drivers::{DriverOp, LayoutDriver, ReplayOp};
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::layout_scratch::NO_ARRANGE_SRC;
use crate::layout::measured::Measured;
use crate::layout::text::text_shape_input::TextShapeInput;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::system::{RunMeasure, TextRunSlot};
use glam::Vec2;
use std::mem;

/// One layer's measure/arrange walk: the engine it mutates, the tree it reads, the
/// text arena and the column block it fills. Built per layer by
/// [`LayoutEngine::run`]. `tree` and `interned_text` are shared references to
/// frozen input; `engine` and `out` stay private behind the accessors.
#[derive(Debug)]
pub(crate) struct LayoutPass<'a> {
    engine: &'a mut LayoutEngine,
    pub(super) tree: &'a Tree,
    pub(super) interned_text: &'a InternedText<'a>,
    out: &'a mut LayerLayout,
    /// The page position of the inner box whose children are being placed. Drivers
    /// place in local coordinates; [`Self::arrange`] adds this once.
    origin: Vec2,
}

/// Where a node arranged into `slot`, in the inner box at page position `origin`,
/// renders. [`LayoutPass::arrange`] and a translated replay both go through here,
/// which is why the replay is bit-exact.
#[inline]
fn place(origin: Vec2, slot: Rect, margin: Spacing) -> Rect {
    Rect {
        min: origin + slot.min,
        size: slot.size,
    }
    .deflated_by(margin)
}

/// One inner box a translated replay is inside: where its subtree ends, and its
/// page position.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReplayOrigin {
    end: usize,
    origin: Vec2,
}

impl<'a> LayoutPass<'a> {
    /// Min-content intrinsic on one axis: the smallest this node can shrink to
    /// without breaking a rigid descendant (Fixed widget, explicit `min_size`, longest
    /// unbreakable word). The lower bound `AxisSlot::resolve` clamps Hug/Fill down to,
    /// cached per (node, axis, slot). Zero on a Fixed axis, which ignores it; skipping
    /// the query keeps a Fixed leaf from triggering a subtree walk each frame.
    fn intrinsic_min(&mut self, node: NodeId, layout: LayoutCore, axis: Axis) -> f32 {
        if axis.main_sizing(layout.size).fixed_value().is_some() {
            return 0.0;
        }
        self.intrinsic(node, axis, LenReq::MinContent)
    }

    /// Measure the children of a per-axis-hug panel (ZStack / Canvas) and return the
    /// extent that covers them. `offset` is where the panel places a child (zero for
    /// a ZStack, the declared position for a Canvas). Every axis offers the child the
    /// room left past the offset (what `arrange` will hand it, or on a Hug axis the
    /// most the panel can grow to), since offering more makes a wrapping child report
    /// a height for a width it won't get. A bounded axis reports the child's own
    /// extent; a Hug axis folds the offset back in.
    pub(super) fn measure_per_axis_hug(
        &mut self,
        node: NodeId,
        inner_avail: Size,
        mut offset: impl FnMut(&Tree, NodeId) -> Vec2,
    ) -> Measured {
        let tree = self.tree;
        let hug = tree.records.layout()[node.idx()].size.hug_mask();
        // The child holds while the room past its position stays past its range.
        let room_from = |from: f32, offset: f32| if from > 0.0 { from + offset } else { 0.0 };
        let mut max = Measured::ZERO;
        for c in tree.active_children(node) {
            let at = offset(tree, c);
            // A hug axis grows to cover where the child was put; a bounded one reports the
            // child's extent alone. The floor follows the extent.
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

    pub(super) const fn new(
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
            origin: Vec2::ZERO,
        }
    }
}

/// Scratch and column access: one method per thing a driver legitimately touches.
impl LayoutPass<'_> {
    #[inline]
    pub(super) fn desired(&self, node: NodeId) -> Size {
        self.engine.scratch.desired[node.idx()]
    }

    /// What this node measured to, as a parent places it ([`Placed::of`]).
    #[inline]
    pub(super) fn placed(&self, node: NodeId) -> Placed {
        Placed::of(self.desired(node), self.engine.scratch.floor[node.idx()])
    }

    /// Grid's per-depth track scratch and durable hug pool, handed back whole because
    /// `grid` disjoint-borrows the halves in one expression.
    #[inline]
    pub(super) const fn grid_mut(&mut self) -> &mut GridContext {
        &mut self.engine.scratch.grid
    }

    #[inline]
    pub(super) const fn grid_track_state_mut(&mut self) -> &mut GridTrackStore {
        &mut self.engine.scratch.grid.track_state
    }

    #[inline]
    pub(super) const fn stack_scratch_mut(&mut self) -> &mut StackScratch {
        &mut self.engine.scratch.stack
    }

    #[inline]
    pub(super) const fn wrap_scratch_mut(&mut self) -> &mut WrapScratch {
        &mut self.engine.scratch.wrap
    }

    /// Measured content extent of a scroll viewport, written by `Scroll::measure` and
    /// read by `Scrollbars::arrange` to size its thumbs.
    #[inline]
    pub(super) fn scroll_content(&self, node: NodeId) -> Size {
        self.out.scroll_content[node.idx()]
    }

    #[inline]
    pub(super) fn set_scroll_content(&mut self, node: NodeId, content: Size) {
        self.out.scroll_content[node.idx()] = content;
    }

    /// Anchor this node and every descendant at a zero-size rect at `anchor`, what a
    /// collapsed subtree gets. Walks the contiguous pre-order span, no recursion.
    #[inline]
    pub(super) fn zero_subtree(&mut self, node: NodeId, anchor: Vec2) {
        let start = node.idx();
        self.engine.scratch.local[start] = anchor;
        let end = self.tree.subtree_end_of(start);
        self.out.rect[start..end].fill(Rect {
            min: self.origin + anchor,
            size: Size::ZERO,
        });
    }

    /// This node's arranged rect, as `arrange` left it.
    #[inline]
    pub(super) fn rect(&self, node: NodeId) -> Rect {
        self.out.rect[node.idx()]
    }

    /// Fold a closed measure span into this run's probe; the spans open around the
    /// root walk, so they close through the pass.
    #[inline]
    pub(super) fn note_measure(&mut self, span: PhaseSpan) {
        self.engine.scratch.counters.add_measure(span);
    }

    #[inline]
    pub(super) fn note_arrange(&mut self, span: PhaseSpan) {
        self.engine.scratch.counters.add_arrange(span);
    }

    /// Outer intrinsic on `axis` under content-sizing `req`; forwards to
    /// [`LayoutEngine::intrinsic`].
    #[inline]
    pub(super) fn intrinsic(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
        self.engine
            .intrinsic(self.tree, node, axis, req, self.interned_text)
    }

    #[inline]
    pub(super) fn intrinsic_range(&mut self, node: NodeId, axis: Axis) -> IntrinsicRange {
        self.engine
            .intrinsic_range(self.tree, node, axis, self.interned_text)
    }
}

impl LayoutPass<'_> {
    /// Bottom-up measure dispatcher; drivers call back here to recurse. Stores each
    /// node's resolved size and its floor (kept by the cache for the subtree's next hit).
    pub(super) fn measure(&mut self, node: NodeId, available: Size) -> Measured {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        let available_q = MeasureCache::available_key(available);
        self.engine.scratch.available_q[node.idx()] = available_q;
        // `is_collapsed`, not `!is_visible`: a `Hidden` node keeps its slot, so its extent
        // is still measured (a text leaf shapes its run). The container-text pass in
        // `LayoutEngine::run` is the opposite: its runs are paint-only.
        if layout.meta.visibility().is_collapsed() {
            self.engine.scratch.desired[node.idx()] = Size::ZERO;
            self.engine.scratch.floor[node.idx()] = Size::ZERO;
            self.engine.scratch.stable_from[node.idx()] = Size::ZERO;
            return Measured::ZERO;
        }

        // Measure-cache short-circuit for any non-leaf node: same `WidgetId`, same rolled
        // subtree hash and an `available` the last measure holds under restore the whole
        // subtree's `desired` and text shapes from last frame's snapshot. The offer check
        // guards a parent resize, since `Hug` and `Fill` can read their offer.
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
                // The subtree hash includes child count, so a length mismatch means the rollup
                // is broken.
                debug_assert_eq!(curr_end, tree.subtree_end_of(curr_start));
                self.engine.scratch.desired[curr_start..curr_end].copy_from_slice(hit.desired);
                // Every node of the subtree, so a descendant arranged at its cached size replays
                // even when the root, at a new size, must dispatch.
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
                // The root is recorded under this offer, not the one restored with its subtree: a
                // root slot keyed on a stale offer fails `MeasureCache::matches_forest` and
                // rebuilds every frame.
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

        // Derive `inner_avail`, dispatch to the driver, and fold content and floor into a
        // margin-inclusive `desired`; see `AxisSlot::resolve_node`.
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

    /// Dispatch one driver measure against the derived `inner_avail`, returning raw
    /// content size and floor. Called once per `measure` (`AxisSlot::resolve_node` says
    /// why a Fill axis growing past `available` needs no re-measure). The match is
    /// `DriverOp::dispatch`, shared with [`Self::arrange`] and `IntrinsicQuery::walk`.
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
    /// (margin-inclusive); stores `rect` for each visited node. A collapsed subtree is
    /// zeroed here at its slot, so drivers needn't mention it; only the two stacks,
    /// whose main-axis cursor a collapsed child must not advance, test for it.
    pub(super) fn arrange(&mut self, node: NodeId, slot: Rect) {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        if layout.meta.visibility().is_collapsed() {
            self.zero_subtree(node, slot.min);
            return;
        }
        self.engine.scratch.local[node.idx()] = slot.min;
        let rendered = place(self.origin, slot, layout.margin);
        let mode = LayoutMode::from(layout.meta);
        if ReplayOp.dispatch(mode) && self.replay_arranged(node, rendered) {
            return;
        }
        self.out.rect[node.idx()] = rendered;
        let inner = layout.inner_rect(rendered);
        let parent = mem::replace(&mut self.origin, inner.min);
        ArrangeOp {
            pass: self,
            node,
            inner: inner.size,
        }
        .dispatch(mode);
        self.origin = parent;
    }

    /// Replay a measure-cache-hit subtree's arranged rects instead of re-running the
    /// drivers; `false` means arrange normally. Sound because arrange's only output is
    /// `out.rect` and the slot origins, and the measure hit proves authoring and
    /// `desired` identical, so arrange is a pure function of the slot. That holds for
    /// drivers whose arrange stays inside their subtree; the caller gates on
    /// [`LayoutDriver::ARRANGE_DEPENDS_ONLY_ON_SLOT`].
    ///
    /// Two of three slot outcomes replay:
    ///
    /// - **Unchanged** rendered rect: a straight `copy_from_slice`.
    /// - **Translated** (same size, moved origin): each rect is rebuilt from its slot
    ///   origin with the same adds [`Self::arrange`] makes, landing bit for bit where
    ///   a cold arrange puts it.
    /// - **Resized**: bails, since a new size redistributes `Fill` children.
    ///
    /// Indexing is safe: the destination range comes from the current tree and the
    /// source is keyed by `WidgetId`, so a subtree that moved still replays.
    #[inline]
    fn replay_arranged(&mut self, node: NodeId, rendered: Rect) -> bool {
        let base = self.engine.scratch.arrange_src[node.idx()];
        if base == NO_ARRANGE_SRC {
            return false;
        }
        let tree = self.tree;
        let start = node.idx();
        let end = tree.subtree_end_of(start);
        let engine = &mut *self.engine;
        let Arranged { rects, locals } = engine.cache.arranged(base as usize, end - start);
        if rects[0].size != rendered.size {
            return false;
        }
        // For the next capture alone; the root's own origin is this frame's.
        if engine.scratch.cache_rebuild {
            engine.scratch.local[start + 1..end].copy_from_slice(&locals[1..]);
        }
        let dst = &mut self.out.rect[start..end];
        if rendered.min == rects[0].min {
            engine.scratch.counters.arrange_copied();
            dst.copy_from_slice(rects);
            return true;
        }
        engine.scratch.counters.arrange_translated();
        let layouts = tree.records.layout();
        let origins = &mut engine.scratch.replay_origins;
        origins.clear();
        origins.push(ReplayOrigin {
            end,
            origin: layouts[start].inner_rect(rendered).min,
        });
        dst[0] = rendered;
        let mut i = start + 1;
        while i < end {
            while origins.last().is_some_and(|open| i >= open.end) {
                origins.pop();
            }
            let parent = origins
                .last()
                .expect("the replay root encloses every node")
                .origin;
            let layout = layouts[i];
            let local = locals[i - start];
            let sub_end = tree.subtree_end_of(i);
            if layout.meta.visibility().is_collapsed() {
                dst[i - start..sub_end - start].fill(Rect {
                    min: parent + local,
                    size: Size::ZERO,
                });
                i = sub_end;
                continue;
            }
            // The size is last frame's, which a translation leaves as it was; `place`
            // contributes the corner alone.
            let size = rects[i - start].size;
            let placed = Rect {
                min: place(parent, Rect { min: local, size }, layout.margin).min,
                size,
            };
            dst[i - start] = placed;
            if sub_end > i + 1 {
                origins.push(ReplayOrigin {
                    end: sub_end,
                    origin: layout.inner_rect(placed).min,
                });
            }
            i += 1;
        }
        true
    }

    /// Shape every text run `runs` yields for `node`, append them to the frame's flat
    /// buffer, and stamp the covering span. Returns the largest run's content size, its
    /// height as the floor, and the widths every run answers the same under.
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
        // Only this node knows the count it just recorded, so reuse rows above it are
        // dropped here; a node that filled every ordinal has none above.
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

    /// A leaf's content is its shaped text, wrapped against the offered width.
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
    inner: Size,
}

impl DriverOp for ArrangeOp<'_, '_> {
    type Output = ();

    fn run<D: LayoutDriver>(self, payload: D::Payload) {
        D::arrange(self.pass, self.node, payload, self.inner);
    }

    fn leaf(self) {}
}
