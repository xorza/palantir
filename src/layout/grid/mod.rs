//! Grid driver: the measure / arrange / intrinsic entry points the
//! layout engine dispatches a `LayoutMode::Grid` node to.
//!
//! The track-sizing solve itself is
//! [`AxisScratch`](axis_scratch::AxisScratch); the state it reads and
//! writes lives in [`GridContext`](grid_context::GridContext).

use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::pass::LayoutPass;
use crate::layout::types::layout_mode::GridDefId;
use crate::primitives::interned_text::InternedText;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

mod arranging;
mod axis_scratch;
pub(crate) mod grid_context;
mod grid_depth_stack;
mod grid_scratch;
pub(crate) mod grid_track_store;
mod measuring;

use crate::layout::grid::arranging::arrange_inner;
use crate::layout::grid::measuring::measure_inner;

/// WPF-style grid measure. Resolves Fixed tracks, walks children once
/// feeding each `Σ spanned-track sizes` (or `∞` if any spanned track is
/// unresolved — the WPF infinity trick → child reports intrinsic), then
/// resolves Hug tracks from span-1 children's desired sizes. Star tracks
/// contribute 0 to the grid's content size — final star sizes only resolve
/// in arrange. The full constraint solver is documented on
/// [`AxisScratch::resolve_axis`].
///
/// Per-depth scratch (`AxisScratch` columns) lives in `grid.depth_stack`
/// and gets clobbered by sibling grids between this measure and the
/// matching arrange. Hug sizes therefore live in `grid.track_state`
/// (`GridTrackStore`), keyed by `GridDef` index, durable for the whole
/// layout pass. Both are heap-resident and capacity-retained across
/// frames; no fixed track-count limit.
pub(super) fn measure(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    idx: GridDefId,
    inner_avail: Size,
) -> Size {
    let depth = pass.grid_mut().depth_stack.enter();
    let result = measure_inner(pass, node, idx, depth, inner_avail);
    pass.grid_mut().depth_stack.exit();
    result
}

pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect, idx: GridDefId) {
    let depth = pass.grid_mut().depth_stack.enter();
    arrange_inner(pass, node, inner, idx, depth);
    pass.grid_mut().depth_stack.exit();
}

/// Intrinsic size of a Grid: per-track contribution aggregated from
/// span-1 cells, summed across tracks plus gaps. Answers "what would
/// the Grid prefer to be on this axis?" so callers can read it without
/// running `measure`.
///
/// Per-track contribution mirrors `Track`'s `Sizing` interpretation:
/// - `Fixed(v)`: contributes `v` clamped to `[Track.min, Track.max]`.
/// - `Hug`: starts at `Track.min`, grown by span-1 cells' intrinsic on
///   the same axis, clamped to `[Track.min, Track.max]`.
/// - `Fill(_)`: same content floor as Hug; weight is ignored until
///   distribution.
///
/// Span > 1 cells are excluded, matching `measure`.
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: GridDefId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let def = tree.grid_defs[usize::from(idx)];
    // An empty dimension means no cells, so the grid measures to
    // `Size::ZERO` (see `measure_inner`); its intrinsic must match on
    // *both* axes — a declared `Fixed` track on the non-empty axis
    // contributes nothing when there's nothing to place in it.
    if def.cols.len == 0 || def.rows.len == 0 {
        return IntrinsicRange::ZERO;
    }
    let (track_span, gap) = match axis {
        Axis::X => (def.cols, def.col_gap),
        Axis::Y => (def.rows, def.row_gap),
    };
    let tracks = &tree.grid_tracks[track_span.range()];
    let n_tracks = tracks.len();

    let wants_min = query.includes(LenReq::MinContent);
    let wants_max = query.includes(LenReq::MaxContent);
    let base = layout.grid_track_aggregator().len();
    let min_base = base;
    let max_base = base + usize::from(wants_min) * n_tracks;
    let slot_count = (usize::from(wants_min) + usize::from(wants_max)) * n_tracks;
    layout
        .grid_track_aggregator()
        .resize(base + slot_count, 0.0);
    for (i, t) in tracks.iter().enumerate() {
        let initial = t
            .size
            .fixed_value()
            .map_or(t.min, |value| value.clamp(t.min, t.max));
        if wants_min {
            layout.grid_track_aggregator()[min_base + i] = initial;
        }
        if wants_max {
            layout.grid_track_aggregator()[max_base + i] = initial;
        }
    }

    for c in tree.active_children(node) {
        let cell_span = tree.bounds(c).grid.track_span(axis);
        if cell_span.len != 1 {
            continue;
        }
        let track_idx = cell_span.start as usize;
        let t = &tracks[track_idx];
        if t.size.fixed_value().is_some() {
            continue;
        }
        let child = query.child(layout, tree, c, axis, interned_text);
        if wants_min {
            let slot = &mut layout.grid_track_aggregator()[min_base + track_idx];
            *slot = slot.max(t.content_floor(child.min));
        }
        if wants_max {
            let slot = &mut layout.grid_track_aggregator()[max_base + track_idx];
            *slot = slot.max(t.content_floor(child.max));
        }
    }

    let gaps = gap * n_tracks.saturating_sub(1) as f32;
    let mut range = IntrinsicRange::ZERO;
    if wants_min {
        range.min = layout.grid_track_aggregator()[min_base..min_base + n_tracks]
            .iter()
            .sum::<f32>()
            + gaps;
    }
    if wants_max {
        range.max = layout.grid_track_aggregator()[max_base..max_base + n_tracks]
            .iter()
            .sum::<f32>()
            + gaps;
    }
    layout.grid_track_aggregator().truncate(base);
    range
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_support {
    use crate::layout::grid::axis_scratch::{AxisScratch, HugRanges};
    use crate::layout::types::track::Track;

    /// Grid's Phase-3 Fill distributor over the same `(weight, floor,
    /// cap)` triples `stack::test_support::distribute_fill` takes, with no
    /// Fixed or Hug tracks and no gap so `total` is the whole leftover.
    pub(crate) fn distribute_fill(items: &[(f32, f32, f32)], total: f32) -> Vec<f32> {
        let tracks: Vec<Track> = items
            .iter()
            .map(|&(weight, _, cap)| Track::fill_weight(weight).max(cap))
            .collect();
        let floors: Vec<f32> = items.iter().map(|&(_, floor, _)| floor).collect();
        let unused_max = vec![0.0; items.len()];
        let mut axis = AxisScratch::default();
        axis.reset(items.len());
        axis.resolve_axis(
            &tracks,
            HugRanges {
                min: &floors,
                max: &unused_max,
            },
            total,
            0.0,
            false,
        );
        axis.sizes.clone()
    }
}
