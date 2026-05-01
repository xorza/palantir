use super::LayoutEngine;
use crate::primitives::{Rect, Size};
use crate::tree::{NodeId, Tree};

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Pass `INFINITY` on both axes during measure
/// so `Fill` children fall back to intrinsic — "fill the rest" is meaningless
/// when children can overlap. Content size = `max(child_pos + child_desired)`
/// per axis, so a `Hug` Canvas grows to the union of placed rects.
pub(super) fn measure(layout: &mut LayoutEngine, tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let pos = tree.node(c).element.position;
        let d = layout.measure(tree, c, child_avail);
        max_w = max_w.max(pos.x + d.w);
        max_h = max_h.max(pos.y + d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `measure`.
pub(super) fn arrange(layout: &mut LayoutEngine, tree: &mut Tree, node: NodeId, inner: Rect) {
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = tree.node(c).desired;
        let pos = tree.node(c).element.position;
        let child_rect = Rect {
            min: inner.min + pos,
            size: d,
        };
        layout.arrange(tree, c, child_rect);
    }
}
