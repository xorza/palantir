//! What a grid asks of its children, and the size that falls out.

use crate::layout::drivers::grid::grid_context::GridContext;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::GridDefId;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;
use crate::scene::tree::node_id::NodeId;

pub(super) fn measure_inner(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    idx: GridDefId,
    depth: usize,
    inner_avail: Size,
) -> Measured {
    let tree = pass.tree;
    let def = tree.grid_defs[usize::from(idx)];
    let row_tracks = &tree.grid_tracks[def.rows.range()];
    let col_tracks = &tree.grid_tracks[def.cols.range()];
    let n_rows = row_tracks.len();
    let n_cols = col_tracks.len();
    let gaps = tree.panel(node).gaps;
    let row_gap = gaps.line_gap();
    let col_gap = gaps.gap();
    let scratch = pass.grid_mut().depth_stack.at(depth);
    scratch.col.reset_for(n_cols);
    scratch.row.reset_for(n_rows);
    pass.grid_track_state_mut().debug_assert_unmeasured(idx);

    if n_rows == 0 || n_cols == 0 {
        // Recurse with `Size::ZERO` so leaves still push `ShapedText` entries: the
        // cascade asserts one per text record even for a zero rect.
        for c in tree.children(node).map(|c| c.id) {
            pass.measure(c, Size::ZERO);
        }
        return Measured::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells. This is the
    // second walk over `active_children` (`Grid::intrinsic` made the first);
    // intrinsics are memoized per (node, axis, req), so the min half is a scratch
    // read. This fold seeds the durable per-track store the resolver then clamps.

    // Resolve the col axis without measuring children, so cells get a committed
    // width before they shape (else wrap text in Hug cols shapes at INF and never
    // wraps). Skip when no column is content-floor-sensitive: Hug cols need `min`
    // and `max`, Fill cols only `min`, Fixed neither.
    let any_content_floor_col = col_tracks
        .iter()
        .any(|t| t.size.is_hug() || t.size.fill_weight().is_some());
    if any_content_floor_col {
        for c in tree.active_children(node) {
            let cell = tree.bounds(c).grid;
            if cell.col_span != 1 {
                continue;
            }
            let t = &col_tracks[cell.col as usize];
            let i = cell.col as usize;
            if t.size.is_hug() {
                let range = pass.intrinsic_range(c, Axis::X);
                let cols = pass.grid_track_state_mut().ranges_mut(idx, Axis::X);
                cols.min[i] = cols.min[i].max(range.min);
                cols.max[i] = cols.max[i].max(range.max);
            } else if t.size.fill_weight().is_some() {
                let min = pass.intrinsic(c, Axis::X, LenReq::MinContent);
                let cols = pass.grid_track_state_mut().ranges_mut(idx, Axis::X);
                cols.min[i] = cols.min[i].max(min);
            }
        }
    }

    // Resolve column widths, giving every cell a committed `available.w`. Whether
    // cells see the resolved Fill width or `INFINITY` depends on the *grid's*
    // sizing: a Hug grid's slot is unknown here, so cells stay unbounded on Fill
    // columns lest row heights commit to an unrelated width; for `Fill` / `Fixed`
    // grids measure's `inner_avail.w` matches arrange's, so cells get the width
    // they will arrange at.
    let grid_sizing = tree.records.layout()[node.idx()].size;
    let grid_sizing_w = grid_sizing.w();
    let grid_sizing_h = grid_sizing.h();
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.at(depth);
        s.col.resolve_axis(
            col_tracks,
            track_state.ranges(idx, Axis::X),
            inner_avail.w,
            col_gap,
            !grid_sizing_w.is_hug(),
        );
        // Stash col sizes for arrange's reuse path.
        track_state.record_resolution(idx, Axis::X, inner_avail.w, &s.col.sizes);
        s.row.resolve_fixed(row_tracks);
    }

    // Phase 2: measure cells at resolved col widths. Rows are unresolved except
    // Fixed, so cells get INF on the row axis. Collapsed children are skipped:
    // `resize_for` zeroed `desired` and arrange anchors them via `zero_subtree`.
    for c in tree.active_children(node) {
        let cell = tree.bounds(c).grid;

        let avail = {
            let s = pass.grid_mut().depth_stack.at(depth);
            // `known_span_size` returns INFINITY if any spanned col is unresolved.
            // Fill cols stay unresolved so their cells get INF: Fill is finalized
            // at arrange, and a different measure width would commit row heights
            // arrange does not honor.
            let avail_w = s.col.known_span_size(cell.track_span(Axis::X), col_gap);
            let avail_h = s.row.known_span_size(cell.track_span(Axis::Y), row_gap);
            Size::new(avail_w, avail_h)
        };

        let d = pass.measure(c, avail);

        // A row's content range, read off the measure: min is the child's floor,
        // max what it wants. Both are *measured*, not Y intrinsics, because the
        // column solve already committed the width. The min is the floor, not
        // `d.size.h`: a scrollable child's desired height is not its minimum, and
        // unwritten (0.0) a cramped Hug row would collapse to `Track.min`.
        // Multi-row spans are skipped, their height being distributed.
        if cell.row_span == 1 {
            let row = cell.row as usize;
            let sizing = row_tracks[row].size;
            if sizing.is_hug() || sizing.fill_weight().is_some() {
                let rows = pass.grid_track_state_mut().ranges_mut(idx, Axis::Y);
                rows.min[row] = rows.min[row].max(d.floor.h);
                if sizing.is_hug() {
                    rows.max[row] = rows.max[row].max(d.size.h);
                }
            }
        }
    }

    // Resolve row heights. The row `resolved` marking is inert here (its reader
    // already ran and arrange re-resolves); only the recorded `sizes` matter.
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.at(depth);
        s.row.resolve_axis(
            row_tracks,
            track_state.ranges(idx, Axis::Y),
            inner_avail.h,
            row_gap,
            !grid_sizing_h.is_hug(),
        );
        track_state.record_resolution(idx, Axis::Y, inner_avail.h, &s.row.sizes);
    }

    // Returned content size: non-Fill track sizes + gaps. Fill claims leftover at
    // arrange; `AxisSlot::resolve` floors this at the Grid intrinsic, which
    // includes Fill content. The floor sums each track at its least: a Fixed
    // track's size, a Hug track's content floor.
    let GridContext {
        depth_stack,
        track_state,
        ..
    } = pass.grid_mut();
    let s = depth_stack.at(depth);
    let total_w = sum_non_fill(col_tracks, &s.col.sizes) + col_gap.gaps_between(n_cols);
    let total_h = sum_non_fill(row_tracks, &s.row.sizes) + row_gap.gaps_between(n_rows);
    let floor_w = floor_non_fill(
        col_tracks,
        track_state.ranges(idx, Axis::X).min,
        &s.col.sizes,
    ) + col_gap.gaps_between(n_cols);
    let floor_h = floor_non_fill(
        row_tracks,
        track_state.ranges(idx, Axis::Y).min,
        &s.row.sizes,
    ) + row_gap.gaps_between(n_rows);
    // Cells are offered their column widths and, down, Fixed rows or nothing, so
    // the grid holds while its Fixed and Hug tracks do; Fill sizes reach neither,
    // except Fill columns a non-Hug grid commits.
    let commits_fill_cols =
        !grid_sizing_w.is_hug() && col_tracks.iter().any(|t| t.size.fill_weight().is_some());
    let stable_from = Size::new(
        if commits_fill_cols {
            Measured::AT_OFFER_ONLY
        } else {
            s.col.stable_from
        },
        s.row.stable_from,
    );
    Measured {
        size: Size::new(total_w, total_h),
        floor: Size::new(floor_w, floor_h),
        stable_from,
    }
}

/// What [`sum_non_fill`] sums at each track's least; never more than the sum.
fn floor_non_fill(tracks: &[Track], content_min: &[f32], sizes: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(content_min)
        .zip(sizes)
        .map(|((t, &min), &size)| {
            if t.size.fill_weight().is_some() {
                0.0
            } else if t.size.is_hug() {
                t.content_floor(min)
            } else {
                size
            }
        })
        .sum()
}

fn sum_non_fill(tracks: &[Track], sizes: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(sizes.iter())
        .map(|(t, &s)| {
            if t.size.fill_weight().is_some() {
                0.0
            } else {
                s
            }
        })
        .sum()
}
