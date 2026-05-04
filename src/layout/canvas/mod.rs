use super::support::{child_avail_per_axis_hug, zero_subtree};
use super::{Axis, LayoutEngine, LenReq};
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextMeasurer;
use crate::tree::{Child, NodeId, Tree};

#[cfg(test)]
mod tests;

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Per-axis available width: pass `inner`
/// when Canvas itself is constrained (Fill / Fixed) so children that need
/// a finite slot to commit cell widths (e.g. Grid's Phase-1 column
/// resolution, wrap text reshaping) get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where `inner` would
/// trigger recursive sizing of Fill children. Same per-axis pattern Stack
/// uses on its cross axis. Content size =
/// `max(child_pos + child_desired)` per axis.
pub(crate) fn measure(
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
    // Active children only: a collapsed child at (100,100) must not
    // inflate the canvas's content size. `desired` is already ZERO for
    // collapsed children (reset at the top of `run`); arrange zeros
    // their subtrees regardless.
    for c in tree.children_active(node) {
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
pub(crate) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect) {
    for child in tree.children_with_state(node) {
        let c = match child {
            Child::Collapsed(c) => {
                zero_subtree(layout, tree, c, inner.min);
                continue;
            }
            Child::Active(c) => c,
        };
        let d = layout.scratch.desired[c.index()];
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
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let mut max = 0.0_f32;
    for c in tree.children_active(node) {
        let pos = tree.read_extras(c).position;
        max = max.max(axis.main_v(pos) + layout.intrinsic(tree, c, axis, req, text));
    }
    max
}
