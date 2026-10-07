//! Which content sizes one intrinsic query asks for, and its walk.

use crate::layout::drivers::DriverOp;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic;
use crate::layout::intrinsic::IntrinsicOp;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk;
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// Which content sizes one query asks for: a single [`LenReq`], or both in one recursion (`None`).
///
/// **A runtime field on purpose.** A `const RANGE: bool` was tried; the mode is constant per query tree so the branch predicts perfectly, while the second monomorphization doubles the recursion's code. Removing it made `grid/intrinsic/forced_miss` ~9% faster. Re-specializing needs a measurement that says otherwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery {
    single_req: Option<LenReq>,
}

impl IntrinsicQuery {
    /// Max over non-collapsed children's outer intrinsic on `axis`, each shifted by `offset`; only Canvas folds in declared positions, other drivers call [`Self::children_max_at_origin`].
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
    /// [`Self::children_max`] for drivers that place children at the container origin (all but Canvas).
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

    /// The query covering exactly the requested halves, or `None`; lets a caller that discards one half narrow the recursion.
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

    /// This child's intrinsic under the same query; halves not asked for come back `0.0`, harmless inside an [`IntrinsicRange::requested`] loop.
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

    /// Outer intrinsic on `axis`: content + padding + margin, respecting `Sizing` and `min_size` / `max_size`; the other axis rides along when the walk covered it ([`IntrinsicWalk`]). Pure function of the subtree; this is the cache miss path.
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

        // Hug and Fill both report content-driven intrinsic (`AxisSlot::resolve` with `available = INFINITY` makes Fill fall back to `content_plus_padding`); Fixed short-circuits and never reads it, so skip the query.
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
