//! Which content sizes one intrinsic query asks for, and the walk that
//! answers it.

use crate::layout::axis::Axis;
use crate::layout::driver::DriverOp;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic;
use crate::layout::intrinsic::IntrinsicOp;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// Which content sizes one query asks for: a single [`LenReq`], or both
/// in one recursion (`None`).
///
/// **A runtime field on purpose.** This was a `const RANGE: bool`
/// threaded through eleven items across six modules, on the theory that
/// specializing kept the recursive path free of per-node mode branches.
/// Measured on `caches`' intrinsic arms, that theory is backwards: the
/// mode is constant for the whole of one query tree, so the branch is
/// predicted every time, while the second monomorphization doubles the
/// code the recursion walks through. Removing it made
/// `grid/intrinsic/forced_miss` ~9% faster (33.7 → 30.7 µs measure, mean
/// of seven interleaved rounds, distributions non-overlapping), with
/// `measure`/`heavy`/`broad` forced-miss arms moving the same direction.
/// Re-specializing needs a measurement that says otherwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery {
    single_req: Option<LenReq>,
}

impl IntrinsicQuery {
    /// Max over non-collapsed children's outer intrinsic on `axis`, each
    /// child's contribution shifted by `offset`.
    ///
    /// Drivers whose own size on an axis is "the largest child wants this
    /// much" (ZStack, Stack cross-axis, WrapStack) call
    /// [`Self::children_max_at_origin`] — Canvas is the one that folds in
    /// each child's declared position. Same closure-parameter shape the measure side uses for the
    /// identical split (`LayoutPass::measure_per_axis_hug`, shared by
    /// `ZStack::measure` and `Canvas::measure`).
    pub(crate) fn children_max(
        self,
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
        mut offset: impl FnMut(&Tree, NodeId) -> f32,
    ) -> IntrinsicRange {
        let mut range = IntrinsicRange::ZERO;
        for c in tree.active_children(node) {
            let child = self.child(layout, tree, c, axis, interned_text);
            let child_offset = offset(tree, c);
            for (req, slot) in range.requested(self) {
                *slot = slot.max(child.get(req) + child_offset);
            }
        }
        range
    }
    /// [`Self::children_max`] for drivers that place children at the
    /// container origin — every one except Canvas.
    #[inline]
    pub(crate) fn children_max_at_origin(
        self,
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        self.children_max(layout, tree, node, axis, interned_text, |_, _| 0.0)
    }

    pub(crate) const fn single(req: LenReq) -> Self {
        Self {
            single_req: Some(req),
        }
    }

    pub(crate) const fn range() -> Self {
        Self { single_req: None }
    }

    /// The query covering exactly the requested halves, or `None` when
    /// neither survives. Lets a caller that discards one half — a scroll
    /// on a panned axis — narrow the recursion instead of computing a
    /// value it will throw away.
    pub(crate) const fn of(min: bool, max: bool) -> Option<Self> {
        match (min, max) {
            (true, true) => Some(Self::range()),
            (true, false) => Some(Self::single(LenReq::MinContent)),
            (false, true) => Some(Self::single(LenReq::MaxContent)),
            (false, false) => None,
        }
    }

    #[inline]
    pub(crate) const fn includes(self, req: LenReq) -> bool {
        match self.single_req {
            Some(single) => single as u8 == req as u8,
            None => true,
        }
    }

    /// This child's intrinsic under the same query.
    ///
    /// Halves the query didn't ask for come back as `0.0`. Read the
    /// result through [`IntrinsicRange::get`] inside an
    /// [`IntrinsicRange::requested`] loop and that can't bite — the two
    /// iterate the same set.
    #[inline]
    pub(crate) fn child(
        self,
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        engine.intrinsic_query(tree, node, axis, self, interned_text)
    }

    /// Outer intrinsic on `axis`: content + padding + margin, respecting the
    /// node's `Sizing` override and `min_size` / `max_size` clamps. The other
    /// axis rides along whenever the walk covered it — see [`IntrinsicWalk`].
    ///
    /// Pure function of the subtree at `node`. Engine caches the result; this
    /// function is the cache miss path.
    pub(crate) fn walk(
        self,
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicWalk {
        let layout = tree.records.layout()[node.idx()];
        if layout.meta.visibility().is_collapsed() {
            return IntrinsicWalk::one_axis(IntrinsicRange::ZERO);
        }

        // Hug + Fill both report content-driven intrinsic: Fill in intrinsic
        // context returns its content's intrinsic, ignoring weight —
        // `AxisSlot::resolve` with `available = INFINITY` enforces exactly that
        // (Fill falls back to `content_plus_padding`). Skip the content query
        // for Fixed: `AxisSlot::resolve` short-circuits Fixed and never reads
        // `content_plus_padding`.
        let content = if axis.main_sizing(layout.size).fixed_value().is_some() {
            IntrinsicWalk::one_axis(IntrinsicRange::ZERO)
        } else {
            IntrinsicOp {
                engine,
                tree,
                node,
                axis,
                query: self,
                interned_text,
            }
            .dispatch(LayoutMode::from(layout.meta))
        };

        let bounds = tree.bounds(node);
        IntrinsicWalk {
            answered: intrinsic::outer(layout, bounds, axis, self, content.answered),
            sibling: content
                .sibling
                .map(|raw| intrinsic::outer(layout, bounds, axis.other(), self, raw)),
        }
    }
}
