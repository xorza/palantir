use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{TextCtx, measure_per_axis_hug, zero_subtree};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::primitives::{rect::Rect, size::Size};

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
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    // Active children only: a collapsed child at (100,100) must not
    // inflate the canvas's content size. `desired` is already ZERO for
    // collapsed children (reset at the top of `run`); arrange zeros
    // their subtrees regardless.
    measure_per_axis_hug(layout, tree, node, inner_avail, tc, out, |tree, c, d| {
        let pos = tree.position_of(c);
        Size::new(pos.x + d.w, pos.y + d.h)
    })
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `measure`.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    out: &mut Layout,
) {
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let d = layout.scratch.desired[c.idx()];
        let pos = tree.position_of(c);
        let child_rect = Rect {
            min: inner.min + pos,
            size: d,
        };
        layout.arrange(tree, c, child_rect, out);
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
    tc: &TextCtx<'_>,
) -> f32 {
    let mut max = 0.0_f32;
    for c in tree.active_children(node) {
        let pos = tree.position_of(c);
        max = max.max(axis.main_v(pos) + layout.intrinsic(tree, c, axis, req, tc));
    }
    max
}
