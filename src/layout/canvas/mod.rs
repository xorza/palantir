use super::{Axis, LayoutEngine, LenReq, child_avail_per_axis_hug, zero_subtree};
use crate::primitives::{Rect, Size};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

#[cfg(test)]
mod tests;

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Per-axis available width: pass `inner`
/// when Canvas itself is constrained (Fill / Fixed) so children that need
/// to know their slot for Step B's column resolution get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where `inner` would
/// trigger recursive sizing of Fill children. Same per-axis pattern Stack
/// uses on its cross axis. Content size =
/// `max(child_pos + child_desired)` per axis.
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
    for c in tree.children(node) {
        // Skip collapsed children outright. `desired` is reset to ZERO at
        // the top of `run`, so no measure call is needed; arrange will
        // zero the subtree's rects, so they must not grow the bbox here
        // either (otherwise a collapsed child at (100,100) would inflate
        // the panel).
        if tree.is_collapsed(c) {
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
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let d = layout.desired[c.index()];
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
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            continue;
        }
        let pos = tree.read_extras(c).position;
        max = max.max(axis.main_v(pos) + layout.intrinsic(tree, c, axis, req, text));
    }
    max
}
