//! What a grid asks of its children, and the size that falls out.

use crate::layout::axis::Axis;
use crate::layout::grid::arranging::known_span_size;
use crate::layout::grid::resolving::resolve_axis;
use crate::layout::grid::{AxisScratch, GridContext, HugKind, reset_hugs_for};
use crate::layout::intrinsic::LenReq;
use crate::layout::pass::LayoutPass;
use crate::layout::types::layout_mode::GridDefId;
use crate::layout::types::track::Track;
use crate::primitives::size::Size;
use crate::scene::tree::record::NodeId;

pub(super) fn measure_inner(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    idx: GridDefId,
    depth: usize,
    inner_avail: Size,
) -> Size {
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
    reset_hugs_for(pass, idx);

    if n_rows == 0 || n_cols == 0 {
        // Recurse with `Size::ZERO` so leaves still take the Leaf measure arm
        // and push `ShapedText` entries for every `ShapeRecord::Text` —
        // the cascade walks shape records and asserts a matching shaped
        // entry per text record, regardless of whether the rect is zero.
        // Skipping the walk breaks `text_reshape_skipped_when_unchanged`.
        for c in tree.children(node).map(|c| c.id) {
            pass.measure(c, Size::ZERO);
        }
        return Size::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
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
                let (cols_min, cols_max) = pass.grid_track_state_mut().slice_mut_pair(idx, Axis::X);
                cols_min[i] = cols_min[i].max(range.min);
                cols_max[i] = cols_max[i].max(range.max);
            } else if t.size.fill_weight().is_some() {
                let min = pass.intrinsic(c, Axis::X, LenReq::MinContent);
                let cols_min = pass
                    .grid_track_state_mut()
                    .slice_mut(idx, Axis::X, HugKind::Min);
                cols_min[i] = cols_min[i].max(min);
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
        resolve_axis(
            &mut s.col,
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
        resolve_fixed(&mut s.row, row_tracks);
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
            let avail_w = known_span_size(
                &s.col.sizes,
                &s.col.resolved,
                cell.track_span(Axis::X),
                col_gap,
            );
            // Rows: only Fixed is known yet; Hug and Fill are unresolved
            // → INF (WPF intrinsic trick), as before.
            let avail_h = known_span_size(
                &s.row.sizes,
                &s.row.resolved,
                cell.track_span(Axis::Y),
                row_gap,
            );
            Size::new(avail_w, avail_h)
        };

        let d = pass.measure(c, avail);

        // Row Hug accumulates from cell's measured height. Row min-content
        // could come from a Y intrinsic query, but it'd be the single-line
        // height — the wrapped height (in `desired.h`) is what actually
        // matters. For Fill rows, the same `d.h` is the min-content
        // floor used by `resolve_axis` Phase 3 to prevent collapse
        // below a rigid descendant (matches Stack's freeze-loop floor).
        // Skip multi-row spans: their height is distributed across rows,
        // not attributable to one row.
        if cell.row_span == 1 {
            let tracks = pass.grid_track_state_mut();
            let row = cell.row as usize;
            let sizing = row_tracks[row].size;
            if sizing.is_hug() {
                let hug_max = tracks.slice_mut(idx, Axis::Y, HugKind::Max);
                hug_max[row] = hug_max[row].max(d.h);
            } else if sizing.fill_weight().is_some() {
                let hug_min = tracks.slice_mut(idx, Axis::Y, HugKind::Min);
                hug_min[row] = hug_min[row].max(d.h);
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
        resolve_axis(
            &mut s.row,
            row_tracks,
            track_state.ranges(idx, Axis::Y),
            inner_avail.h,
            row_gap,
            !grid_sizing_h.is_hug(),
        );
        track_state.record_resolution(idx, Axis::Y, inner_avail.h, &s.row.sizes);
    }

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // claims leftover at arrange; `resolve_sizing` separately floors this
    // raw answer at the Grid intrinsic, which includes Fill content.
    let s = pass.grid_mut().depth_stack.at(depth);
    let total_w =
        sum_non_fill(col_tracks, &s.col.sizes) + col_gap * n_cols.saturating_sub(1) as f32;
    let total_h =
        sum_non_fill(row_tracks, &s.row.sizes) + row_gap * n_rows.saturating_sub(1) as f32;
    Size::new(total_w, total_h)
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

/// Phase 1 of [`resolve_axis`], also run standalone by `measure_inner`
/// before the per-cell loop so `known_span_size` reads Fixed rows as
/// resolved while Hug and Fill rows are still unknown. Returns the total
/// extent the Fixed tracks consumed, which is what `resolve_axis` needs
/// and the standalone caller ignores. Callers reset `a` first — both do,
/// via `AxisScratch::reset` or `resolve_axis`'s own `fill`/`clear`.
pub(super) fn resolve_fixed(a: &mut AxisScratch, tracks: &[Track]) -> f32 {
    let mut consumed = 0.0;
    for (i, t) in tracks.iter().enumerate() {
        if let Some(value) = t.size.fixed_value() {
            a.sizes[i] = value.clamp(t.min, t.max);
            a.resolved.insert(i);
            consumed += a.sizes[i];
        }
    }
    consumed
}
