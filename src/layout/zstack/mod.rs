use crate::layout::axis::Axis;
use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::axis_placement::AxisPlacement;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange};
use crate::layout::pass::LayoutPass;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_text::InternedText;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// Intrinsic size of a ZStack: max over children on the queried axis.
/// Children stack at the same origin, so the parent hugs the largest
/// child.
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    query.children_max_at_origin(layout, tree, node, axis, interned_text)
}

/// ZStack: children all at the same position (top-left of inner rect).
/// Per-axis available width: pass `inner` when the ZStack itself is
/// constrained (Fill / Fixed) so children — including grids that need
/// a finite slot to commit cell widths (e.g. Grid's Phase-1 column
/// resolution) — get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where passing `inner`
/// would create the recursive "ZStack hugs its own Fill child" loop.
/// Same per-axis pattern Stack uses on its cross axis.
///
/// Content size = `max(child desired)` per axis, so the panel hugs the
/// largest child (cross-axis fall-back when ZStack is Hug).
pub(super) fn measure(pass: &mut LayoutPass<'_>, node: NodeId, inner_avail: Size) -> Size {
    pass.measure_per_axis_hug(node, inner_avail, |_, _, d| d)
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y` (with the ZStack's
/// `child_align` as fallback when child's own axis is `Auto`).
/// Defaults pin to top-left unless the child has `Sizing::fill` — then `Auto`
/// falls back to stretch on that axis.
pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect) {
    let tree = pass.tree;
    let parent_child_align = tree.panel(node).child_align;
    let layouts = tree.records.layout();
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            pass.zero_subtree(c, inner.min);
            continue;
        }
        let i = c.idx();
        let s = layouts[i];
        let bounds = tree.bounds(c);
        let mut d = pass.desired(c);
        if matches!(LayoutMode::from(s.meta), LayoutMode::Scroll(_)) {
            // Scroll content sizes its Hug wrapper, but its viewport clips to the slot.
            d = d.min(inner.size);
        }

        let align = AxisAlignPair::resolve(&s, parent_child_align);
        pass.arrange(c, AxisPlacement::arrange_rect(align, &s, bounds, d, inner));
    }
}

#[cfg(test)]
mod tests;
