use super::axis::Axis;
use super::intrinsic::LenReq;
use super::layoutengine::LayoutEngine;
use super::support::{TextCtx, child_avail_per_axis_hug, zero_subtree};
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::primitives::{rect::Rect, size::Size};
use glam::Vec2;

#[cfg(test)]
mod tests;

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Per-axis available width: pass `inner`
/// when Canvas itself is constrained (Fill / Fixed) so children that need
/// a finite slot to commit cell widths (e.g. Grid's Phase-1 column
/// resolution, wrap text reshaping) get a meaningful constraint. Pass
/// `INFINITY` only on Hug axes, where `inner` would trigger recursive
/// sizing of Fill children. Same per-axis pattern Stack uses on its
/// cross axis.
///
/// Content size = `bbox.max - bbox.min` per axis where `bbox` is the
/// union of `(child.pos, child.pos + child.desired)` across active
/// children with the canvas origin `(0,0)` always included. Negative
/// positions therefore *grow* the canvas on the leading side rather
/// than being silently ignored — used by canvas-style scopes (node
/// graphs) where a draggable item can move past origin. With all
/// positions ≥ 0 the floor folds to `(0,0)` and behavior matches the
/// classic "max of child rects" rule, so existing positive-only
/// callers are unaffected. Arrange places children at the unshifted
/// `inner.min + pos`, so negatively-positioned children render at
/// negative scroll-content coords; the enclosing `Scroll` reads the
/// canvas's `bbox.min` from `LayoutScratch::content_origin` (routed
/// through `MeasureCache` so cache hits don't lose it) and extends
/// its offset clamp range on the leading side so those children are
/// reachable without visually jumping siblings.
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let style = tree.records.layout()[node.idx()];
    let child_avail = child_avail_per_axis_hug(style.size, inner_avail);
    let mut bb_min = Vec2::ZERO;
    let mut bb_max = Vec2::ZERO;
    // Active children only: a collapsed child at (100,100) must not
    // inflate the canvas's content size. `desired` is already ZERO for
    // collapsed children (reset at the top of `run`); arrange zeros
    // their subtrees regardless.
    for c in tree.active_children(node) {
        let d = layout.measure(tree, c, child_avail, tc, out);
        let pos = tree.position_of(c);
        bb_min.x = bb_min.x.min(pos.x);
        bb_min.y = bb_min.y.min(pos.y);
        bb_max.x = bb_max.x.max(pos.x + d.w);
        bb_max.y = bb_max.y.max(pos.y + d.h);
    }
    // Publish the leading-edge offset so the enclosing Scroll's
    // measure can extend its offset clamp on the negative side.
    // `Vec2::ZERO` when every position is non-negative — Scroll then
    // sees no leading slack and behaves identically to pre-bbox
    // canvases. The cache round-trips this slot via `SubtreeArenas`,
    // so a cache hit on this subtree still surfaces the right value.
    layout.scratch.content_origin[node.idx()] = bb_min;
    Size::new(bb_max.x - bb_min.x, bb_max.y - bb_min.y)
}

/// Each child gets a slot at `inner.min + style.position`, sized per
/// its desired (intrinsic) size. Negative `position` values therefore
/// render at scroll-content coords `< 0`; the enclosing scroll's
/// extended offset clamp (see `scroll::arrange`) lets the user pan
/// there without visually shifting sibling nodes.
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

/// Intrinsic size of a Canvas: `bbox.max - bbox.min` on the queried
/// axis with the canvas origin always included in the bbox. Matches
/// how `measure` computes the canvas's content size.
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    tc: &TextCtx<'_>,
) -> f32 {
    let mut min = 0.0_f32;
    let mut max = 0.0_f32;
    for c in tree.active_children(node) {
        let pos = axis.main_v(tree.position_of(c));
        let extent = layout.intrinsic(tree, c, axis, req, tc);
        min = min.min(pos);
        max = max.max(pos + extent);
    }
    max - min
}
