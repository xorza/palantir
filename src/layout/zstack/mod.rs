use super::{LayoutEngine, place_axis, resolved_axis_align, zero_subtree};
use crate::primitives::{Rect, Size};
use crate::text::CosmicMeasure;
use crate::tree::{NodeId, Tree};

#[cfg(test)]
mod tests;

/// ZStack: children all at the same position (top-left of inner rect).
/// Pass `INFINITY` on both axes during measure so `Fill` children fall back to
/// intrinsic — otherwise the `Hug` panel would size to its own `Fill` children
/// (recursive). Content size = `max(child desired)` per axis, so the panel
/// hugs the largest child.
pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    mut text: Option<&mut CosmicMeasure>,
) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = layout.measure(tree, c, child_avail, text.as_deref_mut());
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
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let d = layout.desired(c);
        let s = *tree.layout(c);

        let (h_align, v_align) = resolved_axis_align(&s, parent_child_align);
        let (w, x_off) = place_axis(h_align, s.size.w, d.w, inner.size.w, false);
        let (h, y_off) = place_axis(v_align, s.size.h, d.h, inner.size.h, false);

        let child_rect = Rect::new(inner.min.x + x_off, inner.min.y + y_off, w, h);
        layout.arrange(tree, c, child_rect);
    }
}
