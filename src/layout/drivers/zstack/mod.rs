//! The stacking driver: every child measures against and arranges into the same slot, so they overlap.

use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::drivers::LayoutDriver;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::scene::tree::node_id::NodeId;
use glam::{BVec2, Vec2};

#[derive(Clone, Copy, Debug)]
pub(super) struct ZStack {
    node: NodeId,
    /// The axes with no give: a scroll's panned axes, where content takes what it measured.
    rigid: BVec2,
}

impl ZStack {
    pub(super) const fn new(node: NodeId) -> Self {
        Self {
            node,
            rigid: BVec2::FALSE,
        }
    }

    /// This stack with no give on the axes `rigid` sets.
    pub(super) const fn rigid_on(self, rigid: BVec2) -> Self {
        Self { rigid, ..self }
    }
}

impl LayoutDriver for ZStack {
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Children all sit at the inner rect's top-left and are offered the inner extent on both axes, Hug ones included, so grids and wrapping text get a finite constraint whenever the ZStack has one. A Hug ZStack resolves to `min(content, available)`; a Fill child reports its content at measure (`AxisSlot::resolve`), so hugging it doesn't feed back (as Stack does on its cross axis).
    ///
    /// Content size is `max(child desired)` per axis.
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        pass.measure_per_axis_hug(self.node, inner_avail, |_, _| Vec2::ZERO)
    }

    /// Each child gets a slot in `inner` sized by its `Sizing` and placed by `align_x` / `align_y`, falling back to the ZStack's `child_align` when `Auto`. Defaults to top-left; a `Sizing::fill` child's `Auto` stretches.
    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        let slot = Rect {
            min: Vec2::ZERO,
            size: inner,
        };
        let tree = pass.tree;
        let parent_child_align = tree.panel(self.node).child_align;
        for child in tree.children(self.node) {
            let c = child.id;
            let placed = pass.placed(c).rigid_on(self.rigid);
            let align = AxisAlignPair::resolve(placed.layout, parent_child_align);
            pass.arrange(c, placed.arrange_rect(align, slot));
        }
    }

    /// Intrinsic size: max over children on the queried axis.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        pass.children_max_at_origin(self.node, query)
    }
}

#[cfg(test)]
mod tests;
