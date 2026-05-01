use super::{Axis, LayoutEngine, LenReq, zero_subtree};
use crate::primitives::{Rect, Size};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

#[cfg(test)]
mod tests;

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Pass `INFINITY` on both axes during measure
/// so `Fill` children fall back to intrinsic — "fill the rest" is meaningless
/// when children can overlap. Content size = `max(child_pos + child_desired)`
/// per axis, so a `Hug` Canvas grows to the union of placed rects.
pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    text: &mut TextMeasurer,
) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            // Match arrange: collapsed children don't participate in the bbox.
            // Without this skip, a collapsed child at (100, 100) would still
            // grow the panel by its position even though arrange zeroes it.
            layout.measure(tree, c, child_avail, text);
            continue;
        }
        let pos = tree.read_extras(c).position;
        let d = layout.measure(tree, c, child_avail, text);
        max_w = max_w.max(pos.x + d.w);
        max_h = max_h.max(pos.y + d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `measure`.
pub(super) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect) {
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let d = layout.desired(c);
        let pos = tree.read_extras(c).position;
        let child_rect = Rect {
            min: inner.min + pos,
            size: d,
        };
        layout.arrange(tree, c, child_rect);
    }
}

/// Intrinsic size of a Canvas: max over `(child.position +
/// child.intrinsic)` on the queried axis. Matches how `measure` computes
/// the canvas's content size.
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let mut max = 0.0_f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            continue;
        }
        let pos = tree.read_extras(c).position;
        let off = match axis {
            Axis::X => pos.x,
            Axis::Y => pos.y,
        };
        max = max.max(off + layout.intrinsic(tree, c, axis, req, text));
    }
    max
}
