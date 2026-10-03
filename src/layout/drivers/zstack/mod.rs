//! The stacking driver: every child measures against the same slot and
//! arranges into it, so they overlap rather than flow.

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
    /// [`LayoutDriver::arrange`], with the children given no give on the
    /// axes `rigid` sets — a scroll's panned axes, where its content
    /// takes what it measured to however small the viewport.
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

    /// ZStack: children all at the same position (top-left of inner rect).
    /// Every child is offered the inner extent on both axes, Hug ones
    /// included, so children — a grid committing cell widths, wrapping
    /// text — get a finite constraint whenever the ZStack has one. A Hug
    /// ZStack resolves to `min(content, available)`, so that extent is the
    /// most it can grow to; a Fill child reports its content at measure
    /// (`AxisSlot::resolve`), so hugging it does not feed back. Same
    /// pattern Stack uses on its cross axis.
    ///
    /// Content size = `max(child desired)` per axis, so the panel hugs the
    /// largest child (cross-axis fall-back when ZStack is Hug).
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        (): Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        pass.measure_per_axis_hug(node, inner_avail, |_, _| Vec2::ZERO)
    }

    /// Each child gets a slot inside `inner`, sized per its own `Sizing` and
    /// positioned per its `align_x` / `align_y` (with the ZStack's
    /// `child_align` as fallback when child's own axis is `Auto`).
    /// Defaults pin to top-left unless the child has `Sizing::fill` — then `Auto`
    /// falls back to stretch on that axis.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, (): Self::Payload, inner: Size) {
        Self::arrange_in(pass, node, inner, BVec2::FALSE);
    }

    /// Intrinsic size of a ZStack: max over children on the queried axis.
    /// Children stack at the same origin, so the parent hugs the largest
    /// child.
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
