use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{TextCtx, children_max_intrinsic, measure_per_axis_hug, zero_subtree};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::types::sizing::Sizing;
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
    let pos_inflates_x = matches!(canvas_size.w(), Sizing::Hug);
    let pos_inflates_y = matches!(canvas_size.h(), Sizing::Hug);
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
    let layouts = tree.records.layout();
    let canvas_size = layouts[node.idx()].size;
    // Hug canvas: don't stretch Fill children; they'd inflate to the
    // canvas's own intrinsic which already counts `position`. Stretch
    // only when the canvas is constrained (Fixed/Fill), where Fill
    // children should fill the canvas's actual slot.
    let stretch_w = !matches!(canvas_size.w(), Sizing::Hug);
    let stretch_h = !matches!(canvas_size.h(), Sizing::Hug);
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let d = layout.scratch.desired[c.idx()];
        let pos = tree.bounds(c).position;
        let s = layouts[c.idx()].size;
        let pick = |sizing: Sizing, des: f32, full: f32, stretch: bool| match sizing {
            Sizing::Fill(_) if stretch => full,
            _ => des,
        };
        let child_rect = Rect {
            min: inner.min + pos,
            size: Size::new(
                pick(s.w(), d.w, inner.size.w, stretch_w),
                pick(s.h(), d.h, inner.size.h, stretch_h),
            ),
        };
        layout.arrange(tree, c, Some(node), child_rect, out);
    }
}

/// Intrinsic size of a Canvas. Mirrors `measure`'s per-axis gating:
/// when the canvas is `Hug` on the queried axis, returns
/// `max(child.position + child.intrinsic)` so Hug-canvas wraps every
/// positioned child; when `Fill` (or `Fixed`, though `Fixed` doesn't
/// reach this branch — see `intrinsic.rs`), drops the positional offset
/// so a `.position(...)` past `available` can't floor `Fill` above what
/// the parent offered.
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    tc: &TextCtx<'_>,
) -> f32 {
    let pos_inflates = matches!(
        axis.main_sizing(tree.records.layout()[node.idx()].size),
        Sizing::Hug
    );
    children_max_intrinsic(layout, tree, node, axis, req, tc, |tree, c| {
        if pos_inflates {
            axis.main_v(tree.bounds(c).position)
        } else {
            0.0
        }
    })
}
