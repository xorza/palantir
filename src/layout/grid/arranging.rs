//! Where a grid puts what it measured.

use crate::layout::axis::Axis;
use crate::layout::grid::GridContext;
use crate::layout::grid::resolving::resolve_or_reuse;
use crate::layout::pass::LayoutPass;
use crate::layout::support::{AxisAlignPair, arrange_axis, resolved_axis_align};
use crate::layout::types::layout_mode::GridDefId;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::record::NodeId;
use fixedbitset::FixedBitSet;
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
    // same `total` (recorded in `hugs.total_used`), copy the persisted
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
            depth_stack, hugs, ..
        } = pass.grid_mut();
        let s = depth_stack.at(depth);
        resolve_or_reuse(
            &mut s.col,
            col_tracks,
            hugs,
            idx,
            Axis::X,
            inner.size.w,
            col_gap,
        );
        resolve_or_reuse(
            &mut s.row,
            row_tracks,
            hugs,
            idx,
            Axis::Y,
            inner.size.h,
            row_gap,
        );
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

        let (slot_x, slot_y, slot_w, slot_h) = {
            let s = pass.grid_mut().depth_stack.at(depth);
            let slot_x = s.col.offsets[cell.col as usize];
            let slot_y = s.row.offsets[cell.row as usize];
            let slot_w = span_size(&s.col.sizes, cell.track_span(Axis::X), col_gap);
            let slot_h = span_size(&s.row.sizes, cell.track_span(Axis::Y), row_gap);
            (slot_x, slot_y, slot_w, slot_h)
        };

        // Grid's default alignment stretches non-Fixed children to their cell.
        let AxisAlignPair { h, v } = resolved_axis_align(&s_node, parent_child_align);
        let x = arrange_axis(Axis::X, h.or_stretch_if_auto(), &s_node, bounds, d, slot_w);
        let y = arrange_axis(Axis::Y, v.or_stretch_if_auto(), &s_node, bounds, d, slot_h);
        let child_rect = Rect {
            min: inner.min + Vec2::new(slot_x + x.offset, slot_y + y.offset),
            size: Size::new(x.size, y.size),
        };
        pass.arrange(c, child_rect);
    }
}

/// Sum of spanned tracks' resolved sizes, or `∞` if any spanned track is not
/// yet resolved (Hug / Fill at measure time). Internal gaps contribute only
/// when the whole span is known. Infinity makes the child fall back to its
/// intrinsic size on that axis (the WPF trick).
pub(super) fn known_span_size(sizes: &[f32], resolved: &FixedBitSet, span: Span, gap: f32) -> f32 {
    // Cells are range-checked against the parent's track counts at record
    // time (`Tree::check_grid_cell`), so `span.range()` is always in
    // bounds here — index directly.
    let mut sum = 0.0;
    for i in span.range() {
        if !resolved.contains(i) {
            return f32::INFINITY;
        }
        sum += sizes[i];
    }
    sum + gap * span.len.saturating_sub(1) as f32
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
