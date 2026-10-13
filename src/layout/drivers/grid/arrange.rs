//! Where a grid puts what it measured.

use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::drivers::grid::Grid;
use crate::layout::drivers::grid::grid_context::GridContext;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use glam::Vec2;

/// [`LayoutDriver::arrange`](crate::layout::drivers::LayoutDriver::arrange) for `grid`, inside its entered depth slot.
pub(super) fn run(grid: Grid, pass: &mut LayoutPass<'_>, inner: Size) {
    let tree = pass.tree;
    let node = grid.node;
    let cols = grid.axis(tree, Axis::X);
    let rows = grid.axis(tree, Axis::Y);
    let scratch = pass.grid_mut().depth_stack.current();
    scratch.col.reset_for(cols.tracks.len());
    scratch.row.reset_for(rows.tracks.len());

    if rows.tracks.is_empty() || cols.tracks.is_empty() {
        for c in tree.children(node).map(|c| c.id) {
            pass.zero_subtree(c, Vec2::ZERO);
        }
        return;
    }

    // Resolve track sizes. Fast path: when measure resolved this axis against the same `total` (`track_state.total_used` is `Some`) and arrange's `inner.size.X` equals measure's `inner_avail.X` (no Stretch grow since), copy the persisted sizes instead of re-solving. Offsets are cheap, so they are always recomputed.
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.current();
        s.col.resolve_or_reuse(cols, track_state, inner.w);
        s.row.resolve_or_reuse(rows, track_state, inner.h);
        s.col.compute_offsets(cols.gap);
        s.row.compute_offsets(rows.gap);
    }

    let parent_child_align = tree.panel(node).child_align;
    for child in tree.children(node) {
        let c = child.id;
        let placed = pass.placed(c);
        let cell = placed.bounds.grid;

        let slot = {
            let s = pass.grid_mut().depth_stack.current();
            Rect {
                min: Vec2::new(
                    s.col.offsets[cell.col as usize],
                    s.row.offsets[cell.row as usize],
                ),
                size: Size::new(
                    s.col.span_size(cell.track_span(Axis::X), cols.gap),
                    s.row.span_size(cell.track_span(Axis::Y), rows.gap),
                ),
            }
        };

        // Grid's default alignment stretches non-Fixed children to their cell.
        let align = AxisAlignPair::resolve(placed.layout, parent_child_align).or_stretch_if_auto();
        pass.arrange(c, placed.arrange_rect(align, slot));
    }
}
