use super::{
    AutoBias, Axis, LayoutEngine, LenReq, child_avail_per_axis_hug, max_child_intrinsic,
    place_two_axis, zero_subtree,
};
use crate::primitives::{Rect, Size};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

#[cfg(test)]
mod tests;

/// Intrinsic size of a ZStack: max over children on the queried axis.
/// Children stack at the same origin, so the parent hugs the largest
/// child.
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    max_child_intrinsic(layout, tree, node, axis, req, text)
}

/// ZStack: children all at the same position (top-left of inner rect).
/// Per-axis available width: pass `inner` when the ZStack itself is
/// constrained (Fill / Fixed) so children — including grids that need
/// to know their slot for Step B's column resolution — get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where passing `inner`
/// would create the recursive "ZStack hugs its own Fill child" loop.
/// Same per-axis pattern Stack uses on its cross axis.
///
/// Content size = `max(child desired)` per axis, so the panel hugs the
/// largest child (cross-axis fall-back when ZStack is Hug).
pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let style = *tree.layout(node);
    let child_avail = child_avail_per_axis_hug(style.size, inner_avail);
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in tree.children_active(node) {
        let d = layout.measure(tree, c, child_avail, text);
        max_w = max_w.max(d.w);
        max_h = max_h.max(d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y` (with the ZStack's
/// `child_align` as fallback when child's own axis is `Auto`).
/// Defaults pin to top-left unless the child has `Sizing::Fill` — then `Auto`
/// falls back to stretch on that axis.
pub(super) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect) {
    let parent_child_align = tree.read_extras(node).child_align;
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let d = layout.desired[c.index()];
        let s = *tree.layout(c);

        let (size, off) = place_two_axis(
            &s,
            parent_child_align,
            d,
            inner.size,
            AutoBias::StretchIfFill,
        );
        let child_rect = Rect {
            min: inner.min + off,
            size,
        };
        layout.arrange(tree, c, child_rect);
    }
}
