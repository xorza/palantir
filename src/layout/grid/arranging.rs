//! Where a grid puts what it measured.

use crate::layout::axis::Axis;
use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::axis_placement::AxisPlacement;
use crate::layout::grid::grid_context::GridContext;
use crate::layout::pass::LayoutPass;
use crate::layout::types::layout_mode::GridDefId;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

pub(super) fn arrange_inner(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    inner: Rect,
    idx: GridDefId,
    depth: usize,
) {
    let tree = pass.tree;
    let def = tree.grid_defs[usize::from(idx)];
    let row_tracks = &tree.grid_tracks[def.rows.range()];
    let col_tracks = &tree.grid_tracks[def.cols.range()];
    let n_rows = row_tracks.len();
    let n_cols = col_tracks.len();
    let row_gap = def.row_gap;
    let col_gap = def.col_gap;
    let scratch = pass.grid_mut().depth_stack.at(depth);
    scratch.col.reset(n_cols);
    scratch.row.reset(n_rows);

    if n_rows == 0 || n_cols == 0 {
        for c in tree.children(node).map(|c| c.id) {
            pass.zero_subtree(c, inner.min);
        }
        return;
    }

    // Resolve track sizes (Fixed + Hug + Fill) and compute offsets.
    // Fast path: when measure already resolved this axis against the
    // same `total` (recorded in `track_state.total_used`), copy the persisted
    // sizes instead of re-running the constraint solver. The path is
    // safe when:
    //   - measure ran for this grid this frame (`total_used` is `Some` —
    //     cache-hit-ancestor descendants keep the `None` that `reset_for`
    //     left, since nothing ever wrote them);
    //   - arrange's `inner.size.X` matches measure's `inner_avail.X`
    //     (no WPF Stretch grow on this axis since measure committed).
    // The `track_offsets` cumulative-sum is cheap relative to
    // `resolve_axis` (O(n_tracks), no constraint solving) so we re-run
    // it unconditionally — keeps the offsets in sync regardless of
    // which path produced `sizes`.
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.at(depth);
        s.col
            .resolve_or_reuse(col_tracks, track_state, idx, Axis::X, inner.size.w, col_gap);
        s.row
            .resolve_or_reuse(row_tracks, track_state, idx, Axis::Y, inner.size.h, row_gap);
        track_offsets(&s.col.sizes, col_gap, &mut s.col.offsets);
        track_offsets(&s.row.sizes, row_gap, &mut s.row.offsets);
    }

    let parent_child_align = tree.panel(node).child_align;
    let layouts = tree.records.layout();
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            pass.zero_subtree(c, inner.min);
            continue;
        }
        let s_node = layouts[c.idx()];
        let bounds = tree.bounds(c);
        let cell = bounds.grid;
        let d = pass.desired(c);

        let slot = {
            let s = pass.grid_mut().depth_stack.at(depth);
            Rect {
                min: inner.min
                    + Vec2::new(
                        s.col.offsets[cell.col as usize],
                        s.row.offsets[cell.row as usize],
                    ),
                size: Size::new(
                    span_size(&s.col.sizes, cell.track_span(Axis::X), col_gap),
                    span_size(&s.row.sizes, cell.track_span(Axis::Y), row_gap),
                ),
            }
        };

        // Grid's default alignment stretches non-Fixed children to their cell.
        let align = AxisAlignPair::resolve(&s_node, parent_child_align).or_stretch_if_auto();
        pass.arrange(
            c,
            AxisPlacement::arrange_rect(align, &s_node, bounds, d, slot),
        );
    }
}

fn track_offsets(sizes: &[f32], gap: f32, out: &mut [f32]) {
    debug_assert_eq!(sizes.len(), out.len());
    let mut acc = 0.0f32;
    for (i, &s) in sizes.iter().enumerate() {
        out[i] = acc;
        acc += s;
        if i + 1 < sizes.len() {
            acc += gap;
        }
    }
}

fn span_size(sizes: &[f32], span: Span, gap: f32) -> f32 {
    // In-bounds by the same record-time cell range check as
    // `known_span_size`.
    let r = span.range();
    let n = r.len();
    let mut total: f32 = sizes[r].iter().sum();
    if n > 1 {
        total += gap * (n - 1) as f32;
    }
    total
}
