//! Grid driver: the measure / arrange / intrinsic entry points for a `LayoutMode::Grid` node; the track solve
//! is [`AxisScratch`](axis_scratch::AxisScratch), its state [`GridContext`](grid_context::GridContext).

use crate::layout::drivers::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::GridDefId;
use crate::primitives::math::num::F32Px;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

mod arrange;
mod axis_scratch;
pub(crate) mod grid_context;
mod grid_depth_stack;
mod grid_scratch;
pub(crate) mod grid_track_store;
mod measure;

use crate::layout::drivers::grid::arrange::arrange_inner;
use crate::layout::drivers::grid::measure::measure_inner;

#[derive(Debug)]
pub(super) struct Grid;

impl LayoutDriver for Grid {
    type Payload = GridDefId;

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// WPF-style grid measure: resolves Fixed tracks, feeds each child the sum of its spanned tracks (`∞` if any
    /// is unresolved, so the child reports intrinsic), then resolves Hug tracks from span-1 children. Star tracks
    /// resolve only in arrange. Per-depth scratch is clobbered by sibling grids between measure and arrange, so
    /// Hug sizes live in `grid.track_state` (`GridTrackStore`) for the whole pass. The solver is documented on
    /// [`AxisScratch::resolve_axis`](crate::layout::drivers::grid::axis_scratch::AxisScratch::resolve_axis).
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        idx: Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        let depth = pass.grid_mut().depth_stack.enter();
        let result = measure_inner(pass, node, idx, depth, inner_avail);
        pass.grid_mut().depth_stack.exit();
        result
    }

    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, idx: Self::Payload, inner: Size) {
        let depth = pass.grid_mut().depth_stack.enter();
        arrange_inner(pass, node, idx, depth, inner);
        pass.grid_mut().depth_stack.exit();
    }

    /// Intrinsic size of a Grid: span-1 cells' per-track contributions summed across tracks plus gaps (span > 1
    /// cells are excluded, as in `measure`); `Fixed` clamps to `[min, max]`, `Hug` and `Fill` share a content floor.
    fn intrinsic(
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        idx: Self::Payload,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        let def = tree.grid_defs[usize::from(idx)];
        // An empty dimension means no cells, so the grid measures `Size::ZERO` (see `measure_inner`) on both axes.
        if def.cols.len == 0 || def.rows.len == 0 {
            return IntrinsicRange::ZERO;
        }
        let gaps = tree.panel(node).gaps;
        let (track_span, gap) = match axis {
            Axis::X => (def.cols, gaps.gap()),
            Axis::Y => (def.rows, gaps.line_gap()),
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

        let gaps = gap.gaps_between(n_tracks);
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
}

#[cfg(test)]
mod tests;
