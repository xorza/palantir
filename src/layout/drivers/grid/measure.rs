//! What a grid asks of its children, and the size that falls out.

use crate::primitives::layout::offer;

use crate::layout::drivers::grid::grid_track_store::{GridAxis, GridTrackStore};
use crate::layout::drivers::grid::{self, Grid};
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::ContentMeasure;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;

/// [`LayoutDriver::measure`](crate::layout::drivers::LayoutDriver::measure) for `grid`.
pub(super) fn run(grid: Grid, pass: &mut LayoutPass<'_>, inner_avail: Size) -> ContentMeasure {
    let tree = pass.tree();
    let node = grid.node;
    let cols = grid.axis(tree, Axis::X);
    let rows = grid.axis(tree, Axis::Y);
    let row_count = rows.tracks.len();
    let col_count = cols.tracks.len();
    pass.grid_mut()
        .track_state
        .debug_assert_unmeasured(grid.def);

    if row_count == 0 || col_count == 0 {
        // Recurse with `Size::ZERO` so leaves still push `ShapedText` entries: the
        // cascade asserts one per text record even for a zero rect.
        for c in tree.children(node).map(|c| c.id) {
            pass.measure(c, Size::ZERO);
        }
        return ContentMeasure::ZERO;
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
        .any(|t| t.sizing().is_hug() || t.sizing().fill_weight().is_some());
    if any_content_floor_col {
        for c in tree.active_children(node) {
            let cell = grid.cell(tree, c);
            if cell.col_span() != 1 {
                continue;
            }
            let i = cell.col() as usize;
            let t = &cols.tracks[i];
            if t.sizing().is_hug() {
                let range = pass.intrinsic_range(c, Axis::X);
                let col_ranges = pass.grid_mut().track_state.ranges_mut(cols.id);
                col_ranges.min[i] = col_ranges.min[i].max(range.min);
                col_ranges.max[i] = col_ranges.max[i].max(range.max);
            } else if t.sizing().fill_weight().is_some() {
                let min = pass.intrinsic(c, Axis::X, LenReq::MinContent);
                let col_ranges = pass.grid_mut().track_state.ranges_mut(cols.id);
                col_ranges.min[i] = col_ranges.min[i].max(min);
            }
        }
        // After every span-1 cell, so a spanning cell grows only what they leave short; spans that
        // cross a Fill column after those that cross none.
        for flexible in [false, true] {
            for c in tree.active_children(node) {
                let span = grid.cell(tree, c).track_span(Axis::X);
                if span.len < 2 || grid::crosses_fill(cols.tracks, span) != flexible {
                    continue;
                }
                let range = pass.intrinsic_range(c, Axis::X);
                let col_ranges = pass.grid_mut().track_state.ranges_mut(cols.id);
                cols.distribute_span(span, range.min, col_ranges.min);
                cols.distribute_span(span, range.max, col_ranges.max);
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
        let tracks = &mut pass.grid_mut().track_state;
        tracks.resolve_axis(cols, inner_avail.w);
        if !grid_sizing_w.is_hug() && inner_avail.w.is_finite() {
            tracks.commit_fill(cols.id, cols.tracks);
        }
        tracks.resolve_fixed(rows.id, rows.tracks);
    }

    // Phase 2: measure cells at resolved col widths. Rows are unresolved except
    // Fixed, so cells get INF on the row axis. Collapsed children are skipped:
    // `resize_for` zeroed `desired` and arrange anchors them via `zero_subtree`.
    for c in tree.active_children(node) {
        let cell = grid.cell(tree, c);

        let avail = {
            let tracks = &pass.grid_mut().track_state;
            // `known_span_size` returns INFINITY if any spanned col is unresolved.
            // Fill cols stay unresolved so their cells get INF: Fill is finalized
            // at arrange, and a different measure width would commit row heights
            // arrange does not honor.
            let avail_w = tracks.known_span_size(cols.id, cell.track_span(Axis::X), cols.gap);
            let avail_h = tracks.known_span_size(rows.id, cell.track_span(Axis::Y), rows.gap);
            Size::new(avail_w, avail_h)
        };

        let d = pass.measure(c, avail);

        {
            let tracks = &mut pass.grid_mut().track_state;
            for (axis, grid_axis) in [(Axis::X, cols), (Axis::Y, rows)] {
                let span = cell.track_span(axis);
                let i = span.start as usize;
                if span.len == 1 && grid_axis.tracks[i].sizing().fill_weight().is_some() {
                    let extent = &mut tracks.extents_mut(grid_axis.id)[i];
                    *extent = extent.max(axis.main(d.size));
                }
            }
        }

        // A row's content range, read off the measure: min is the child's floor,
        // max what it wants. Both are *measured*, not Y intrinsics, because the
        // column solve already committed the width. The min is the floor, not
        // `d.size.h`: a scrollable child's desired height is not its minimum, and
        // unwritten (0.0) a cramped Hug row would collapse to `Track.min`.
        // Multi-row spans wait for the loop below.
        if cell.row_span() == 1 {
            let row = cell.row() as usize;
            let sizing = rows.tracks[row].sizing();
            if sizing.is_hug() || sizing.fill_weight().is_some() {
                let row_ranges = pass.grid_mut().track_state.ranges_mut(rows.id);
                row_ranges.min[row] = row_ranges.min[row].max(d.floor.h);
                if sizing.is_hug() {
                    row_ranges.max[row] = row_ranges.max[row].max(d.size.h);
                }
            }
        }
    }

    // A multi-row cell grows the rows it crosses by what the span-1 rows leave short; spans that
    // cross a Fill row after those that cross none.
    for flexible in [false, true] {
        for c in tree.active_children(node) {
            let span = grid.cell(tree, c).track_span(Axis::Y);
            if span.len < 2 || grid::crosses_fill(rows.tracks, span) != flexible {
                continue;
            }
            let (size, floor) = (pass.desired(c).h, pass.floor(c).h);
            let row_ranges = pass.grid_mut().track_state.ranges_mut(rows.id);
            rows.distribute_span(span, floor, row_ranges.min);
            rows.distribute_span(span, size, row_ranges.max);
        }
    }

    // Resolve row heights. Fill rows are left uncommitted: the row `resolved`
    // marking is inert here (its reader already ran and arrange re-resolves);
    // only the recorded `sizes` matter.
    pass.grid_mut()
        .track_state
        .resolve_axis(rows, inner_avail.h);

    // What a Fill track holds: its single-track cells' extent, with the other tracks at their
    // resolved sizes, grown by the spans that cross a Fill track.
    for axis in [Axis::X, Axis::Y] {
        let grid_axis = match axis {
            Axis::X => cols,
            Axis::Y => rows,
        };
        if !grid::has_fill(grid_axis.tracks) {
            continue;
        }
        pass.grid_mut()
            .track_state
            .hold_resolved_extents(grid_axis.id, grid_axis.tracks);
        for c in tree.active_children(node) {
            let span = grid.cell(tree, c).track_span(axis);
            if span.len < 2 || !grid::crosses_fill(grid_axis.tracks, span) {
                continue;
            }
            let need = axis.main(pass.desired(c));
            let extents = pass.grid_mut().track_state.extents_mut(grid_axis.id);
            grid_axis.distribute_span(span, need, extents);
        }
    }

    // Returned content size: non-Fill track sizes, what the Fill tracks hold
    // ([`fill_contribution`]), and gaps. On every path: a Fill grid offered a finite width
    // shapes its cells at the Fill widths it commits, and a parent that hugs it must hold them,
    // as a stack reports what its Fill children measured to. Fill still claims the leftover at
    // arrange. `AxisSlot::resolve` floors this at the Grid intrinsic. The floor sums each track
    // at its least: a Fixed track's size, a Hug track's content floor.
    let tracks = &pass.grid_mut().track_state;
    let col_gaps = cols.gap.gaps_between(col_count);
    let row_gaps = rows.gap.gaps_between(row_count);
    let grid_sizing_h = tree.records.layout()[node.idx()].size.h();
    let fill_w = cols.fill_share(grid_sizing_w, inner_avail.w, tracks);
    let fill_h = rows.fill_share(grid_sizing_h, inner_avail.h, tracks);
    let total_w = grid::sum_non_fill(cols.tracks, tracks.sizes(cols.id)) + fill_w.extent + col_gaps;
    let total_h = grid::sum_non_fill(rows.tracks, tracks.sizes(rows.id)) + fill_h.extent + row_gaps;
    let floor_w = floor_non_fill(
        cols.tracks,
        tracks.content_min(cols.id),
        tracks.sizes(cols.id),
    ) + col_gaps;
    let floor_h = floor_non_fill(
        rows.tracks,
        tracks.content_min(rows.id),
        tracks.sizes(rows.id),
    ) + row_gaps;
    // Cells are offered their column widths and, down, Fixed rows or nothing, so the grid holds
    // while its tracks do ([`FillShare::stable_from`]); Fill sizes reach no cell, except Fill
    // columns a non-Hug grid commits.
    let commits_fill_cols = !grid_sizing_w.is_hug() && grid::has_fill(cols.tracks);
    let stable_from = Size::new(
        if commits_fill_cols {
            offer::AT_OFFER_ONLY
        } else {
            fill_w.stable_from
        },
        fill_h.stable_from,
    );
    ContentMeasure {
        size: Size::new(total_w, total_h),
        floor: Size::new(floor_w, floor_h),
        stable_from,
    }
}

/// What an axis's Fill tracks add to the grid's measured size, and the least offer it holds from.
#[derive(Clone, Copy, Debug)]
struct FillShare {
    /// What holds each track's measured extent at a shared fraction ([`grid::fill_extent`]), as
    /// CSS Grid §12.7.1 sizes a flexible track in indefinite space and WPF's Grid sums a star
    /// track's `MinSize` into its desired size. Where the grid fills a finite offer, at most what
    /// the tracks resolved to at it, as arrange gives them no more there: a min-clamped track
    /// would raise the shared fraction past the offer.
    extent: f32,
    /// The axis's [`ContentMeasure::stable_from`], the tracks' own and the cap's.
    stable_from: f32,
}

impl GridAxis<'_> {
    /// The share of this axis's tracks, resolved in `store`, on a grid sized `sizing` offered
    /// `avail`.
    fn fill_share(self, sizing: Sizing, avail: f32, store: &GridTrackStore) -> FillShare {
        let Self { tracks, id, .. } = self;
        let held = grid::fill_extent(tracks, store.extents(id));
        let stable_from = store.stable_from(id);
        if sizing.is_hug() || !avail.is_finite() || !grid::has_fill(tracks) {
            return FillShare {
                extent: held,
                stable_from,
            };
        }
        let resolved = grid::sum_fill(tracks, store.sizes(id));
        if held <= resolved {
            // A larger offer gives the Fill tracks no less, so the hold stands from this offer
            // up; a smaller one may cap it.
            FillShare {
                extent: held,
                stable_from: stable_from.max(avail),
            }
        } else {
            FillShare {
                extent: resolved,
                stable_from: offer::AT_OFFER_ONLY,
            }
        }
    }
}

/// What [`grid::sum_non_fill`] sums at each track's least; never more than the sum.
fn floor_non_fill(tracks: &[Track], content_min: &[f32], sizes: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(content_min)
        .zip(sizes)
        .map(|((t, &min), &size)| {
            if t.sizing().fill_weight().is_some() {
                0.0
            } else if t.sizing().is_hug() {
                t.content_floor(min)
            } else {
                size
            }
        })
        .sum()
}
