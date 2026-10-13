//! What a grid asks of its children, and the size that falls out.

use crate::layout::drivers::grid::Grid;
use crate::layout::drivers::grid::grid_context::GridContext;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;

/// [`LayoutDriver::measure`](crate::layout::drivers::LayoutDriver::measure) for `grid`, inside its entered depth slot.
pub(super) fn run(grid: Grid, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
    let tree = pass.tree;
    let node = grid.node;
    let cols = grid.axis(tree, Axis::X);
    let rows = grid.axis(tree, Axis::Y);
    let n_rows = rows.tracks.len();
    let n_cols = cols.tracks.len();
    let scratch = pass.grid_mut().depth_stack.current();
    scratch.col.reset_for(n_cols);
    scratch.row.reset_for(n_rows);
    pass.grid_track_state_mut()
        .debug_assert_unmeasured(grid.def);

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
    let any_content_floor_col = cols
        .tracks
        .iter()
        .any(|t| t.size.is_hug() || t.size.fill_weight().is_some());
    if any_content_floor_col {
        for c in tree.active_children(node) {
            let cell = tree.bounds(c).grid;
            if cell.col_span != 1 {
                continue;
            }
            let i = cell.col as usize;
            let t = &cols.tracks[i];
            if t.size.is_hug() {
                let range = pass.intrinsic_range(c, Axis::X);
                let col_ranges = pass.grid_track_state_mut().ranges_mut(cols.id);
                col_ranges.min[i] = col_ranges.min[i].max(range.min);
                col_ranges.max[i] = col_ranges.max[i].max(range.max);
            } else if t.size.fill_weight().is_some() {
                let min = pass.intrinsic(c, Axis::X, LenReq::MinContent);
                let col_ranges = pass.grid_track_state_mut().ranges_mut(cols.id);
                col_ranges.min[i] = col_ranges.min[i].max(min);
            }
        }
    }

    // Resolve column widths, giving every cell a committed `available.w`. Whether
    // cells see the resolved Fill width or `INFINITY` depends on the *grid's*
    // sizing: a Hug grid's slot is unknown here, so cells stay unbounded on Fill
    // columns lest row heights commit to an unrelated width; for `Fill` / `Fixed`
    // grids measure's `inner_avail.w` matches arrange's, so cells get the width
    // they will arrange at.
    let grid_sizing_w = tree.records.layout()[node.idx()].size.w();
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.current();
        s.col
            .resolve_axis(cols, track_state.ranges(cols.id), inner_avail.w);
        if !grid_sizing_w.is_hug() && inner_avail.w.is_finite() {
            s.col.commit_fill(cols.tracks);
        }
        // Stash col sizes for arrange's reuse path.
        track_state.record_resolution(cols.id, inner_avail.w, &s.col.sizes);
        s.row.resolve_fixed(rows.tracks);
    }

    // Phase 2: measure cells at resolved col widths. Rows are unresolved except
    // Fixed, so cells get INF on the row axis. Collapsed children are skipped:
    // `resize_for` zeroed `desired` and arrange anchors them via `zero_subtree`.
    for c in tree.active_children(node) {
        let cell = tree.bounds(c).grid;

        let avail = {
            let s = pass.grid_mut().depth_stack.current();
            // `known_span_size` returns INFINITY if any spanned col is unresolved.
            // Fill cols stay unresolved so their cells get INF: Fill is finalized
            // at arrange, and a different measure width would commit row heights
            // arrange does not honor.
            let avail_w = s.col.known_span_size(cell.track_span(Axis::X), cols.gap);
            let avail_h = s.row.known_span_size(cell.track_span(Axis::Y), rows.gap);
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
            let sizing = rows.tracks[row].size;
            if sizing.is_hug() || sizing.fill_weight().is_some() {
                let row_ranges = pass.grid_track_state_mut().ranges_mut(rows.id);
                row_ranges.min[row] = row_ranges.min[row].max(d.floor.h);
                if sizing.is_hug() {
                    row_ranges.max[row] = row_ranges.max[row].max(d.size.h);
                }
            }
        }
    }

    // Resolve row heights. Fill rows are left uncommitted: the row `resolved`
    // marking is inert here (its reader already ran and arrange re-resolves);
    // only the recorded `sizes` matter.
    {
        let GridContext {
            depth_stack,
            track_state,
            ..
        } = pass.grid_mut();
        let s = depth_stack.current();
        s.row
            .resolve_axis(rows, track_state.ranges(rows.id), inner_avail.h);
        track_state.record_resolution(rows.id, inner_avail.h, &s.row.sizes);
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
    let s = depth_stack.current();
    let col_gaps = cols.gap.gaps_between(n_cols);
    let row_gaps = rows.gap.gaps_between(n_rows);
    let total_w = sum_non_fill(cols.tracks, &s.col.sizes) + col_gaps;
    let total_h = sum_non_fill(rows.tracks, &s.row.sizes) + row_gaps;
    let floor_w =
        floor_non_fill(cols.tracks, track_state.ranges(cols.id).min, &s.col.sizes) + col_gaps;
    let floor_h =
        floor_non_fill(rows.tracks, track_state.ranges(rows.id).min, &s.row.sizes) + row_gaps;
    // Cells are offered their column widths and, down, Fixed rows or nothing, so
    // the grid holds while its Fixed and Hug tracks do; Fill sizes reach neither,
    // except Fill columns a non-Hug grid commits.
    let commits_fill_cols =
        !grid_sizing_w.is_hug() && cols.tracks.iter().any(|t| t.size.fill_weight().is_some());
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
