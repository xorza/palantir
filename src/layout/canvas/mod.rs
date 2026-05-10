use super::support::{measure_per_axis_hug, zero_subtree};
use super::{Axis, LayoutEngine, LenReq};
use crate::forest::tree::{Child, NodeId, Tree};
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextShaper;

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
    text: &TextShaper,
) -> Size {
    // Active children only: a collapsed child at (100,100) must not
    // inflate the canvas's content size. `desired` is already ZERO for
    // collapsed children (reset at the top of `run`); arrange zeros
    // their subtrees regardless.
    measure_per_axis_hug(layout, tree, node, inner_avail, text, |tree, c, d| {
        let pos = tree.bounds(c).position;
        Size::new(pos.x + d.w, pos.y + d.h)
    })
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `measure`.
pub(crate) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect) {
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let d = layout.scratch.desired[c.index()];
        let pos = tree.bounds(c).position;
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
    text: &TextShaper,
) -> f32 {
    let mut max = 0.0_f32;
    for c in tree.children(node).filter_map(Child::active) {
        let pos = tree.bounds(c).position;
        max = max.max(axis.main_v(pos) + layout.intrinsic(tree, c, axis, req, text));
    }
    max
}
