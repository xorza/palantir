use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{
    AxisAlignPair, children_max_intrinsic, measure_per_axis_hug, place_axis, resolved_axis_align,
    zero_subtree,
};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextShaper;
use glam::Vec2;

#[cfg(test)]
mod tests;

/// Intrinsic size of a ZStack: max over children on the queried axis.
/// Children stack at the same origin, so the parent hugs the largest
/// child.
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &TextShaper,
) -> f32 {
    children_max_intrinsic(layout, tree, node, axis, req, text)
}

/// ZStack: children all at the same position (top-left of inner rect).
/// Per-axis available width: pass `inner` when the ZStack itself is
/// constrained (Fill / Fixed) so children — including grids that need
/// a finite slot to commit cell widths (e.g. Grid's Phase-1 column
/// resolution) — get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where passing `inner`
/// would create the recursive "ZStack hugs its own Fill child" loop.
/// Same per-axis pattern Stack uses on its cross axis.
///
/// Content size = `max(child desired)` per axis, so the panel hugs the
/// largest child (cross-axis fall-back when ZStack is Hug).
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    text: &TextShaper,
    out: &mut Layout,
) -> Size {
    measure_per_axis_hug(layout, tree, node, inner_avail, text, out, |_, _, d| d)
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y` (with the ZStack's
/// `child_align` as fallback when child's own axis is `Auto`).
/// Defaults pin to top-left unless the child has `Sizing::Fill` — then `Auto`
/// falls back to stretch on that axis.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    out: &mut Layout,
) {
    let parent_child_align = tree.panel(node).child_align;
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let d = layout.scratch.desired[c.index()];
        let s = tree.records.layout()[c.index()];

        let AxisAlignPair { h, v } = resolved_axis_align(&s, parent_child_align);
        let x = place_axis(h, s.size.w(), d.w, inner.size.w);
        let y = place_axis(v, s.size.h(), d.h, inner.size.h);
        let child_rect = Rect {
            min: inner.min + Vec2::new(x.offset, y.offset),
            size: Size::new(x.size, y.size),
        };
        layout.arrange(tree, c, child_rect, out);
    }
}
