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
        // Recurse with `Size::ZERO` so leaves still take the Leaf measure arm
        // and push `ShapedText` entries for every `ShapeRecord::Text` —
        // the cascade walks shape records and asserts a matching shaped
        // entry per text record, regardless of whether the rect is zero.
        // Skipping the walk breaks `text_reshape_skipped_when_unchanged`.
        for c in tree.children(node).map(|c| c.id) {
            pass.measure(c, Size::ZERO);
        }
        return Measured::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
    //
    // The second walk over `active_children` this measure — `Grid::intrinsic`
    // made the first, for the grid's own `intrinsic_min` — and not a repeat
    // of it. Per-child intrinsics are memoized per (node, axis, req), so the
    // min half every cell asks for below is a scratch read; the max half is
    // new work that the min-content query never computed. What the two folds
    // produce also differs: that one sums track contributions clamped to
    // `[Track.min, Track.max]` into transient aggregator slots, this one
    // seeds the durable per-track store the resolver then clamps.

    // Resolves the col axis without measuring children — the whole
    // point is to give cells a committed column width before they
    // shape (otherwise wrap text in Hug cols would always shape at INF
    // and never wrap).
    // Skip the span-1 child walk entirely when no column is content-
    // floor-sensitive. Hug cols need both `min` (constraint solver lo)
    // and `max` (constraint solver hi); Fill cols only need `min` so
    // the Phase 3 distributor floors them at their cells' min-content
    // (matching Stack's freeze-loop floor, prevents collapse below a
    // rigid descendant like a Fixed widget or unbreakable word).
    // Fixed cols read neither.
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

    // Resolve column widths now (Fixed + Hug + Fill). Gives every cell a
    // committed `available.w` before it measures.
    //
    // For Fill cols specifically, whether cells should see the resolved
    // Fill width or `INFINITY` depends on the *grid's* sizing on this
    // axis. A Hug grid's final slot is still unknown here: its desired
    // width is resolved later from `sum_non_fill` plus the intrinsic
    // floor that includes Fill content. Cells therefore stay unbounded
    // on Fill columns so row heights cannot commit to the unrelated
    // measure-time available width. For non-Hug grids (`Fill` / `Fixed`),
    // measure's `inner_avail.w` matches arrange's `inner.w`, so Fill cols
    // at measure time give cells the same width they'll get at arrange —
    // wrap text shapes correctly.
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
        // Stash col sizes for arrange's reuse path (skips a redundant
        // `resolve_axis` when the arrange-time slot matches `inner_avail.w`).
        track_state.record_resolution(idx, Axis::X, inner_avail.w, &s.col.sizes);
        // Resolve Fixed rows once before the per-cell loop — values are
        // constant per GridDef and `resolve_fixed` is idempotent, so
        // calling it inside the loop just re-set the same slots.
        s.row.resolve_fixed(row_tracks);
    }

    // Phase 2: measure cells with resolved col widths. Rows are still
    // unresolved (only Fixed is known); cells get INF on row axis as
    // before. Cell desired heights feed row Hug resolution next.
    // Collapsed children skipped — `LayoutScratch::resize_for` already
    // zeroed `desired` for the whole frame, and arrange anchors
    // collapsed subtrees via `zero_subtree`.
    for c in tree.active_children(node) {
        let cell = tree.bounds(c).grid;

        let avail = {
            let s = pass.grid_mut().depth_stack.at(depth);
            // `known_span_size` returns INFINITY if any spanned col is
            // unresolved. After `resolve_axis` ran above, Fixed and Hug
            // cols are marked resolved; Fill cols intentionally stay
            // unresolved so cells in them get INF here — Fill stays
            // finalized at arrange time. Without this, cells in Fill
            // cols would measure at a different width than they're
            // arranged at, and that discrepancy commits row heights
            // based on a width arrange doesn't honor.
            let avail_w = s.col.known_span_size(cell.track_span(Axis::X), col_gap);
            // Rows: only Fixed is known yet; Hug and Fill are unresolved
            // → INF (WPF intrinsic trick), as before.
            let avail_h = s.row.known_span_size(cell.track_span(Axis::Y), row_gap);
            Size::new(avail_w, avail_h)
        };

        let d = pass.measure(c, avail);

        // A row's content range, both ends, read off the measure: the
        // min is the child's floor, the max what it actually wants. Both
        // are *measured* rather than Y intrinsics, because the column
        // solve above already committed the width, and the wrapped height
        // is what the cell will paint.
        //
        // The min is what a cramped total falls back on — `resolve_axis`
        // floors Hug at it (Phase 2) and Fill at it (Phase 3, matching
        // Stack's freeze-loop floor). It is the floor and not `d.size.h`:
        // a scrollable child's desired height is not its minimum, and a
        // row that refused to shrink below it would deny a viewport the
        // slot its grid was capped to. Left unwritten it is 0.0, and a
        // cramped Hug row collapses all the way to `Track.min` where the
        // same content in a column stops at its min-content.
        //
        // Skip multi-row spans: their height is distributed across rows,
        // not attributable to one row.
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

    // Resolve row heights. Shares `resolve_axis` with the col pass, so
    // Phase 4 still runs — but the row `resolved` marking is inert here:
    // its only reader (`known_span_size` in Phase 2) has already run,
    // `resolved` is not part of the persisted arrange state (only `sizes`
    // + `total` are), and arrange's re-resolve rebuilds it from scratch.
    // Only the resolved `sizes` recorded below matter past this point.
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

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // claims leftover at arrange; `AxisSlot::resolve` separately floors this
    // raw answer at the Grid intrinsic, which includes Fill content. The
    // floor sums the same tracks at what each cannot go below: a Fixed
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
    // Cells are offered their column widths and, down, Fixed rows or
    // nothing, so the grid holds while its Fixed and Hug tracks do. Fill
    // track sizes reach neither the content size nor a cell — except Fill
    // columns a non-Hug grid commits, whose widths its cells measure at.
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

/// What [`sum_non_fill`] sums, at each track's least: a Fixed track's
/// resolved size, a Hug track's content floor. Never more than the sum,
/// because the solve never sizes a Hug track below its floor.
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
