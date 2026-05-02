use super::{
    AutoBias, Axis, LayoutEngine, LenReq, child_avail_per_axis_hug, place_axis,
    resolved_axis_align, zero_subtree,
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
    let mut max = 0.0_f32;
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            continue;
        }
        max = max.max(layout.intrinsic(tree, c, axis, req, text));
    }
    max
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
    let child_avail = child_avail_per_axis_hug(layout, tree, node, style.size, inner_avail, text);
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in tree.children(node) {
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
        let d = layout.desired(c);
        let s = *tree.layout(c);

        let (h_align, v_align) = resolved_axis_align(&s, parent_child_align);
        let (w, x_off) = place_axis(
            h_align,
            s.size.w,
            d.w,
            inner.size.w,
            AutoBias::StretchIfFill,
        );
        let (h, y_off) = place_axis(
            v_align,
            s.size.h,
            d.h,
            inner.size.h,
            AutoBias::StretchIfFill,
        );

        let child_rect = Rect::new(inner.min.x + x_off, inner.min.y + y_off, w, h);
        layout.arrange(tree, c, child_rect);
    }
}
