//! [`IntrinsicPass`]: the borrows one on-demand intrinsic query holds, and the
//! three layers that answer it.

use crate::layout::axis_slot::AxisSlot;
use crate::layout::drivers::{DriverOp, LayoutDriver};
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::plane_input::PlaneInput;
use crate::layout::text::text_shape_input::TextShapeInput;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::system::TextRunSlot;

/// One intrinsic query over a subtree: the engine it memoizes into, the tree it
/// reads and the text arena its leaves shape from.
///
/// The query is a pure function of the subtree and must not write the frame's
/// text shapes; holding no [`PlaneLayout`](crate::layout::plane_layout::PlaneLayout)
/// enforces that by construction, where a
/// [`LayoutPass`](crate::layout::pass::LayoutPass) would hold one.
#[derive(Debug)]
pub(crate) struct IntrinsicPass<'a> {
    engine: &'a mut LayoutEngine,
    tree: &'a Tree,
    interned_text: &'a InternedText<'a>,
}

impl<'a> IntrinsicPass<'a> {
    pub(crate) const fn new(engine: &'a mut LayoutEngine, input: PlaneInput<'a>) -> Self {
        Self {
            engine,
            tree: input.tree,
            interned_text: input.interned_text,
        }
    }

    #[inline]
    pub(crate) const fn tree(&self) -> &'a Tree {
        self.tree
    }

    /// Grid's per-track intrinsic aggregator: a bump stack `Grid::intrinsic`
    /// extends, recurses through and truncates back.
    #[inline]
    pub(crate) const fn grid_track_aggregator(&mut self) -> &mut Vec<f32> {
        &mut self.engine.scratch.grid.track_aggregator
    }

    /// Outer (margin-inclusive) size of `node` for the halves `query` asks for.
    ///
    /// A pure function of the subtree at `node`, independent of the parent's
    /// available width and the arranged rect. Three layers answer, cheapest
    /// first: the intra-frame slot array, last frame's snapshot, then a real
    /// subtree walk. Only halves still missing reach the walk; halves not
    /// asked for come back `0.0`, harmless inside an
    /// [`IntrinsicRange::requested`] loop.
    ///
    /// A walk that also covered the other axis (see [`IntrinsicWalk`]) is
    /// recorded there too, keeping `measure`'s pair of min-content queries to
    /// one pass over a leaf's text runs.
    pub(crate) fn query(&mut self, node: NodeId, query: IntrinsicQuery) -> IntrinsicRange {
        let idx = node.idx();
        let mut range = IntrinsicRange::ZERO;
        for (req, slot) in range.requested(query) {
            *slot = self.memo(idx, query.axis(), req).unwrap_or(f32::NAN);
        }
        let Some(walk) = query.retain(|req| range.get(req).is_nan()) else {
            return range;
        };
        self.engine.scratch.counters.intrinsic_computed();
        let computed = self.walk(node, walk);
        if let Some(sibling) = computed.sibling {
            self.record(idx, walk.across(), sibling);
        }
        self.record(idx, walk, computed.answered);
        for (req, slot) in range.requested(walk) {
            *slot = computed.answered.get(req);
        }
        range
    }

    /// One half of [`Self::query`].
    #[inline]
    pub(crate) fn single(&mut self, node: NodeId, axis: Axis, req: LenReq) -> f32 {
        self.query(node, IntrinsicQuery::single(axis, req)).get(req)
    }

    /// Both halves, as Grid's Hug tracks need the content range.
    #[inline]
    pub(crate) fn range(&mut self, node: NodeId, axis: Axis) -> IntrinsicRange {
        self.query(node, IntrinsicQuery::range(axis))
    }

    /// Max over non-collapsed children's outer intrinsic, each shifted by `offset`; only Canvas folds in declared positions, other drivers call [`Self::children_max_at_origin`].
    pub(crate) fn children_max(
        &mut self,
        node: NodeId,
        query: IntrinsicQuery,
        mut offset: impl FnMut(&Tree, NodeId) -> f32,
    ) -> IntrinsicRange {
        let tree = self.tree;
        let mut range = IntrinsicRange::ZERO;
        for c in tree.active_children(node) {
            let child = self.query(c, query);
            let child_offset = offset(tree, c);
            for (req, slot) in range.requested(query) {
                *slot = slot.max(child.get(req) + child_offset);
            }
        }
        range
    }

    /// [`Self::children_max`] for drivers that place children at the container origin (all but Canvas).
    #[inline]
    pub(crate) fn children_max_at_origin(
        &mut self,
        node: NodeId,
        query: IntrinsicQuery,
    ) -> IntrinsicRange {
        self.children_max(node, query, |_, _| 0.0)
    }

    /// A memoized value for one `(node, axis, req)` slot: this frame's slot
    /// array, else last frame's snapshot, which hits even on a resize frame.
    /// The snapshot is read here, at the query site, because a parent
    /// computes `intrinsic_min` before measuring its children, so their own
    /// cache-hit restore comes too late to stop the ancestor recursing through
    /// every unchanged sibling subtree.
    #[inline]
    fn memo(&mut self, idx: usize, axis: Axis, req: LenReq) -> Option<f32> {
        let slot = req.slot(axis);
        let cur = self.engine.scratch.intrinsics[idx][slot];
        if !cur.is_nan() {
            return Some(cur);
        }
        let snapshot = self.snapshot(idx, slot)?;
        self.engine.scratch.intrinsics[idx][slot] = snapshot;
        Some(snapshot)
    }

    /// Cross-frame intrinsic for one slot, or `None` when the node is
    /// ineligible or the snapshot has no value. Only non-leaf nodes are
    /// cacheable (a leaf's is cheap and owns no descriptor). Intrinsics are
    /// independent of `available`, so this checks `subtree_hash` alone and
    /// hits even on a resize frame where `try_lookup` misses.
    #[inline]
    fn snapshot(&self, idx: usize, slot: usize) -> Option<f32> {
        let tree = self.tree;
        if LayoutMode::from(tree.records.layout()[idx].meta) == LayoutMode::Leaf {
            return None;
        }
        self.engine.cache.lookup_root_intrinsic(
            tree.records.widget_id()[idx],
            tree.rollups.layout_subtree[idx],
            slot,
        )
    }

    /// Store the halves `query` names of `found` in this frame's slot array.
    #[inline]
    fn record(&mut self, idx: usize, query: IntrinsicQuery, mut found: IntrinsicRange) {
        for (req, value) in found.requested(query) {
            self.engine.scratch.intrinsics[idx][req.slot(query.axis())] = *value;
        }
    }

    /// Outer intrinsic: content + padding + margin, respecting `Sizing` and `min_size` / `max_size`; the other axis rides along when the walk covered it ([`IntrinsicWalk`]). The cache miss path.
    fn walk(&mut self, node: NodeId, query: IntrinsicQuery) -> IntrinsicWalk {
        let tree = self.tree;
        let layout = tree.records.layout()[node.idx()];
        if layout.meta.visibility().is_collapsed() {
            return IntrinsicWalk::one_axis(IntrinsicRange::ZERO);
        }

        // Hug and Fill both report content-driven intrinsic (`AxisSlot::resolve` with `available = INFINITY` makes Fill fall back to `content_plus_padding`); Fixed short-circuits and never reads it, so skip the query.
        let content = if query
            .axis()
            .main_sizing(layout.size)
            .fixed_value()
            .is_some()
        {
            IntrinsicWalk::one_axis(IntrinsicRange::ZERO)
        } else {
            IntrinsicOp { pass: self, query }.dispatch(node, LayoutMode::from(layout.meta))
        };

        let bounds = tree.bounds(node);
        let outer = |query: IntrinsicQuery, raw: IntrinsicRange| {
            AxisSlot::unbounded(&layout, bounds, query.axis()).outer(query, raw)
        };
        IntrinsicWalk {
            answered: outer(query, content.answered),
            sibling: content.sibling.map(|raw| outer(query.across(), raw)),
        }
    }

    /// Leaf: walks shapes and aggregates. Only `ShapeRecord::Text` contributes non-zero intrinsics;
    /// other shapes are owner-relative paint and don't drive size. Lives here because leaves have no
    /// driver module.
    ///
    /// A run's content demands are `Size`s, so the accumulators are too and both axes fall out of one
    /// pass; see [`IntrinsicWalk`] for what the other buys.
    fn leaf(&mut self, node: NodeId, query: IntrinsicQuery) -> IntrinsicWalk {
        let (tree, interned_text) = (self.tree, self.interned_text);
        let widget_id = tree.records.widget_id()[node.idx()];
        let mut min_content = Size::ZERO;
        let mut max_content = Size::ZERO;
        for ts in TextShapeInput::on_leaf(tree, interned_text, node) {
            let unbounded = self.engine.text.root(
                TextRunSlot {
                    widget_id,
                    ordinal: ts.ordinal,
                },
                ts.shape_request(),
                ts.wrap,
            );
            if query.includes(LenReq::MinContent) {
                min_content = min_content.max(ts.wrap.min_content(&unbounded));
            }
            if query.includes(LenReq::MaxContent) {
                max_content = max_content.max(ts.wrap.max_content(&unbounded));
            }
        }
        let axis = query.axis();
        IntrinsicWalk {
            answered: IntrinsicRange {
                min: axis.main(min_content),
                max: axis.main(max_content),
            },
            sibling: Some(IntrinsicRange {
                min: axis.cross(min_content),
                max: axis.cross(max_content),
            }),
        }
    }
}

/// The one [`DriverOp`] carrying an [`IntrinsicPass`] rather than a
/// [`LayoutPass`](crate::layout::pass::LayoutPass).
#[derive(Debug)]
struct IntrinsicOp<'op, 'a> {
    pass: &'op mut IntrinsicPass<'a>,
    query: IntrinsicQuery,
}

impl DriverOp for IntrinsicOp<'_, '_> {
    type Output = IntrinsicWalk;

    fn run<D: LayoutDriver>(self, driver: D) -> IntrinsicWalk {
        IntrinsicWalk::one_axis(driver.intrinsic(self.pass, self.query))
    }

    fn leaf(self, node: NodeId) -> IntrinsicWalk {
        self.pass.leaf(node, self.query)
    }
}
