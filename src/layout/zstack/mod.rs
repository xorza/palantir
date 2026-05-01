use super::LayoutEngine;
use crate::primitives::{Rect, Size, Visibility};
use crate::tree::{NodeId, Tree};

/// ZStack: children all at the same position (top-left of inner rect).
/// Pass `INFINITY` on both axes during measure so `Fill` children fall back to
/// intrinsic — otherwise the `Hug` panel would size to its own `Fill` children
/// (recursive). Content size = `max(child desired)` per axis, so the panel
/// hugs the largest child.
pub(super) fn measure(layout: &mut LayoutEngine, tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = layout.measure(tree, c, child_avail);
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
pub(super) fn arrange(layout: &mut LayoutEngine, tree: &mut Tree, node: NodeId, inner: Rect) {
    let parent_layout = tree.node(node).element;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            super::zero_subtree(tree, c, inner.min);
            continue;
        }
        let d = tree.node(c).desired;
        let s = tree.node(c).element;

        let (h_align, v_align) = super::resolved_axis_align(&s, &parent_layout);
        let (w, x_off) = super::place_axis(h_align, s.size.w, d.w, inner.size.w, false);
        let (h, y_off) = super::place_axis(v_align, s.size.h, d.h, inner.size.h, false);

        let child_rect = Rect::new(inner.min.x + x_off, inner.min.y + y_off, w, h);
        layout.arrange(tree, c, child_rect);
    }
}
