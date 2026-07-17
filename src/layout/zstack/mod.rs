use crate::forest::tree::Tree;
use crate::forest::tree::node::NodeId;
use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::LenReq;
use crate::layout::support::{
    AxisAlignPair, TextCtx, arrange_axis, children_max_intrinsic, measure_per_axis_hug,
    resolved_axis_align, zero_subtree,
};
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::{rect::Rect, size::Size};
use glam::Vec2;

/// Intrinsic size of a ZStack: max over children on the queried axis.
/// Children stack at the same origin, so the parent hugs the largest
/// child.
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    tc: &TextCtx<'_>,
) -> f32 {
    children_max_intrinsic(layout, tree, node, axis, req, tc)
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
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    measure_per_axis_hug(layout, tree, node, inner_avail, tc, out, |_, _, d| d)
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y` (with the ZStack's
/// `child_align` as fallback when child's own axis is `Auto`).
/// Defaults pin to top-left unless the child has `Sizing::fill` — then `Auto`
/// falls back to stretch on that axis.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    out: &mut Layout,
) {
    let parent_child_align = tree.panel(node).child_align;
    let layouts = tree.records.layout();
    let self_outer = out[layout.active_layer].rect[node.idx()].size;
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let i = c.idx();
        let s = layouts[i];
        let bounds = tree.bounds(c);
        let mut d = layout.scratch.desired[i];
        if s.mode == LayoutMode::Scroll {
            // Scroll content sizes its Hug wrapper, but its viewport clips to the slot.
            d = d.min(inner.size);
        }

        let AxisAlignPair { h, v } = resolved_axis_align(&s, parent_child_align);
        let x = arrange_axis(Axis::X, h, &s, bounds, d, inner.size.w);
        let y = arrange_axis(Axis::Y, v, &s, bounds, d, inner.size.h);
        let child_rect = Rect {
            min: inner.min + Vec2::new(x.offset, y.offset),
            size: Size::new(x.size, y.size),
        };
        layout.arrange(tree, c, self_outer, child_rect, out);
    }
}

#[cfg(test)]
mod tests;
