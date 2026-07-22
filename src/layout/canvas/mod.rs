use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange};
use crate::layout::support::{
    TextCtx, arrange_axis, children_max_intrinsic_offset, measure_per_axis_hug, zero_subtree,
};
use crate::layout::types::align::AxisAlign;
use crate::layout::types::sizing::Sizing;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::node::NodeId;

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Per-axis available width: pass `inner`
/// when Canvas itself is constrained (Fill / Fixed) so children that need
/// a finite slot to commit cell widths (e.g. Grid's Phase-1 column
/// resolution, wrap text reshaping) get a meaningful
/// constraint. Pass `INFINITY` only on Hug axes, where `inner` would
/// trigger recursive sizing of Fill children. Same per-axis pattern Stack
/// uses on its cross axis.
///
/// **Content size per axis depends on the canvas's own sizing on that
/// axis.** A `Hug` axis reports `max(child_pos + child_desired)` so the
/// canvas wraps every positioned child. A `Fill` axis reports
/// `max(child_desired)` — `.position(...)` becomes purely positional and
/// can't inflate the canvas past its available. Without this gating, a
/// child placed at `.position(700, ...)` with size 160 forces a FILL
/// canvas's `intrinsic_min` to 860, which floors FILL above the
/// available and overflows the surface; in the damage diff, the canvas's
/// chrome paint rect then changes every frame the user drags the child,
/// producing `Damage::Full` flicker (the darkroom graph-view bug).
/// Negative positions render outside the canvas's `inner` either way
/// (the loop's running max starts at 0); if you need scrollable
/// negative-origin canvases, see
/// [`crate::widgets::scroll::Scroll::anchor_canvas_origin`] for the
/// userspace pattern (shift positions into positive space and
/// auto-compensate the scroll's offset so visible state stays stable).
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let canvas_size = tree.records.layout()[node.idx()].size;
    let pos_inflates_x = canvas_size.w().is_hug();
    let pos_inflates_y = canvas_size.h().is_hug();
    // Active children only: a collapsed child at (100,100) must not
    // inflate the canvas's content size. `desired` is already ZERO for
    // collapsed children (reset at the top of `run`); arrange zeros
    // their subtrees regardless.
    measure_per_axis_hug(layout, tree, node, inner_avail, tc, out, |tree, c, d| {
        let pos = tree.bounds(c).position;
        let off_x = if pos_inflates_x { pos.x } else { 0.0 };
        let off_y = if pos_inflates_y { pos.y } else { 0.0 };
        Size::new(off_x + d.w, off_y + d.h)
    })
}

/// Each child gets a slot at `inner.min + bounds.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `measure`.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    out: &mut Layout,
) {
    let layouts = tree.records.layout();
    let canvas_size = layouts[node.idx()].size;
    let self_outer = out[layout.active_layer].rect[node.idx()].size;
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let d = layout.scratch.desired[c.idx()];
        let pos = tree.bounds(c).position;
        let child_layout = layouts[c.idx()];
        let bounds = tree.bounds(c);
        let slot_w = if canvas_size.w().is_hug() {
            d.w
        } else {
            inner.size.w
        };
        let slot_h = if canvas_size.h().is_hug() {
            d.h
        } else {
            inner.size.h
        };
        let child_rect = Rect {
            min: inner.min + pos,
            size: Size::new(
                arrange_axis(Axis::X, AxisAlign::Auto, &child_layout, bounds, d, slot_w).size,
                arrange_axis(Axis::Y, AxisAlign::Auto, &child_layout, bounds, d, slot_h).size,
            ),
        };
        layout.arrange(tree, c, self_outer, child_rect, out);
    }
}

/// Intrinsic size of a Canvas. Mirrors `measure`'s per-axis gating:
/// when the canvas is `Hug` on the queried axis, returns
/// `max(child.position + child.intrinsic)` so Hug-canvas wraps every
/// positioned child; when `Fill` (or `Fixed`, though `Fixed` doesn't
/// reach this branch — see `intrinsic.rs`), drops the positional offset
/// so a `.position(...)` past `available` can't floor `Fill` above what
/// the parent offered.
pub(crate) fn intrinsic<const RANGE: bool>(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery<RANGE>,
    tc: &TextCtx<'_>,
) -> IntrinsicRange {
    let pos_inflates = matches!(
        axis.main_sizing(tree.records.layout()[node.idx()].size),
        Sizing::HUG
    );
    children_max_intrinsic_offset(layout, tree, node, axis, query, tc, |tree, c| {
        if pos_inflates {
            axis.main_v(tree.bounds(c).position)
        } else {
            0.0
        }
    })
}

#[cfg(test)]
mod tests;
