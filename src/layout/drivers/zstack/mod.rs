//! The stacking driver: every child measures against and arranges into the same slot, so they overlap.

use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::axis_placement::AxisPlacement;
use crate::layout::drivers::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use glam::{BVec2, Vec2};

#[derive(Debug)]
pub(super) struct ZStack;

impl ZStack {
    /// [`LayoutDriver::arrange`] with no give on the axes `rigid` sets (a scroll's panned axes, where content takes what it measured).
    pub(super) fn arrange_in(pass: &mut LayoutPass<'_>, node: NodeId, inner: Size, rigid: BVec2) {
        let slot = Rect {
            min: Vec2::ZERO,
            size: inner,
        };
        let tree = pass.tree;
        let parent_child_align = tree.panel(node).child_align;
        let layouts = tree.records.layout();
        for child in tree.children(node) {
            let c = child.id;
            let i = c.idx();
            let s = layouts[i];
            let bounds = tree.bounds(c);
            let placed = pass.placed(c).rigid_on(rigid);
            let align = AxisAlignPair::resolve(&s, parent_child_align);
            pass.arrange(
                c,
                AxisPlacement::arrange_rect(align, &s, bounds, placed, slot),
            );
        }
    }
}

impl LayoutDriver for ZStack {
    type Payload = ();

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Children all sit at the inner rect's top-left and are offered the inner extent on both axes, Hug ones included, so grids and wrapping text get a finite constraint whenever the ZStack has one. A Hug ZStack resolves to `min(content, available)`; a Fill child reports its content at measure (`AxisSlot::resolve`), so hugging it doesn't feed back (as Stack does on its cross axis).
    ///
    /// Content size is `max(child desired)` per axis.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        (): Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        pass.measure_per_axis_hug(node, inner_avail, |_, _| Vec2::ZERO)
    }

    /// Each child gets a slot in `inner` sized by its `Sizing` and placed by `align_x` / `align_y`, falling back to the ZStack's `child_align` when `Auto`. Defaults to top-left; a `Sizing::fill` child's `Auto` stretches.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, (): Self::Payload, inner: Size) {
        Self::arrange_in(pass, node, inner, BVec2::FALSE);
    }

    /// Intrinsic size: max over children on the queried axis.
    fn intrinsic(
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        (): Self::Payload,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        query.children_max_at_origin(layout, tree, node, axis, interned_text)
    }
}

#[cfg(test)]
mod tests;
