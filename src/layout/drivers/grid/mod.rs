//! Grid driver: the measure / arrange / intrinsic entry points for a `LayoutMode::Grid` node; the track solve
//! is [`AxisScratch`](axis_scratch::AxisScratch), its state [`GridCtx`](grid_ctx::GridCtx).

use crate::common::span::Span;
use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::grid::axis_scratch::GridAxis;
use crate::layout::drivers::grid::grid_track_store::GridAxisId;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::layout_mode::GridDefId;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

mod arrange;
mod axis_scratch;
pub(crate) mod grid_ctx;
mod grid_depth_stack;
mod grid_scratch;
pub(crate) mod grid_track_store;
mod measure;

#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    node: NodeId,
    def: GridDefId,
}

impl Grid {
    pub(super) const fn new(node: NodeId, def: GridDefId) -> Self {
        Self { node, def }
    }

    /// `child`'s cell held inside this grid's tracks ([`GridCell::clamped_to`]). Only a grid
    /// with tracks on both axes places cells.
    fn cell(self, tree: &Tree, child: NodeId) -> GridCell {
        let def = tree.grid_defs[usize::from(self.def)];
        // A cell addresses at most `u16::MAX` tracks, so a longer list holds it at that.
        let tracks = |span: Span| u16::try_from(span.len).unwrap_or(u16::MAX);
        tree.bounds(child)
            .grid
            .clamped_to(tracks(def.rows), tracks(def.cols))
    }

    /// This grid's tracks on `axis` and the gap between them: columns and `gap`
    /// across, rows and `line_gap` down.
    fn axis(self, tree: &Tree, axis: Axis) -> GridAxis<'_> {
        let def = tree.grid_defs[usize::from(self.def)];
        let gaps = tree.panel(self.node).gaps;
        let (span, gap) = match axis {
            Axis::X => (def.cols, gaps.gap()),
            Axis::Y => (def.rows, gaps.line_gap()),
        };
        GridAxis {
            id: GridAxisId {
                def: self.def,
                axis,
            },
            tracks: &tree.grid_tracks[span.range()],
            gap,
        }
    }
}

impl LayoutDriver for Grid {
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// WPF-style grid measure: resolves Fixed tracks, feeds each child the sum of its spanned tracks (`∞` if any
    /// is unresolved, so the child reports intrinsic), then resolves Hug tracks from span-1 children and the shortfall of spanning ones. Star tracks
    /// resolve only in arrange. Per-depth scratch is clobbered by sibling grids between measure and arrange, so
    /// Hug sizes live in `grid.track_state` (`GridTrackStore`) for the whole pass. The solver is documented on
    /// [`AxisScratch::resolve_axis`](crate::layout::drivers::grid::axis_scratch::AxisScratch::resolve_axis).
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        pass.grid_mut().depth_stack.enter();
        let measured = measure::run(self, pass, inner_avail);
        pass.grid_mut().depth_stack.exit();
        measured
    }

    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        pass.grid_mut().depth_stack.enter();
        arrange::run(self, pass, inner);
        pass.grid_mut().depth_stack.exit();
    }

    /// Intrinsic size of a Grid: span-1 cells' per-track contributions, then each spanning cell's shortfall
    /// spread over the Hug tracks it crosses ([`distribute_span`]), summed across tracks plus gaps, as in
    /// `measure`; `Fixed` clamps to `[min, max]`, `Hug` and `Fill` share a content floor.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        let tree = pass.tree();
        let def = tree.grid_defs[usize::from(self.def)];
        // An empty dimension means no cells, so the grid measures `Size::ZERO` (see `measure::run`) on both axes.
        if def.cols.len == 0 || def.rows.len == 0 {
            return IntrinsicRange::ZERO;
        }
        let axis = query.axis();
        let GridAxis { tracks, gap, .. } = self.axis(tree, axis);
        let n_tracks = tracks.len();

        let wants_min = query.includes(LenReq::MinContent);
        let wants_max = query.includes(LenReq::MaxContent);
        let base = pass.grid_track_aggregator().len();
        let min_base = base;
        let max_base = base + usize::from(wants_min) * n_tracks;
        let slot_count = (usize::from(wants_min) + usize::from(wants_max)) * n_tracks;
        pass.grid_track_aggregator().resize(base + slot_count, 0.0);
        for (i, t) in tracks.iter().enumerate() {
            let min = t.min();
            let initial = t
                .sizing()
                .fixed_value()
                .map_or(min, |value| value.clamp(min, t.max()));
            if wants_min {
                pass.grid_track_aggregator()[min_base + i] = initial;
            }
            if wants_max {
                pass.grid_track_aggregator()[max_base + i] = initial;
            }
        }

        for c in tree.active_children(self.node) {
            let cell_span = self.cell(tree, c).track_span(axis);
            if cell_span.len != 1 {
                continue;
            }
            let track_idx = cell_span.start as usize;
            let t = &tracks[track_idx];
            if t.sizing().fixed_value().is_some() {
                continue;
            }
            let child = pass.query(c, query);
            if wants_min {
                let slot = &mut pass.grid_track_aggregator()[min_base + track_idx];
                *slot = slot.max(t.content_floor(child.min));
            }
            if wants_max {
                let slot = &mut pass.grid_track_aggregator()[max_base + track_idx];
                *slot = slot.max(t.content_floor(child.max));
            }
        }

        for c in tree.active_children(self.node) {
            let cell_span = self.cell(tree, c).track_span(axis);
            if cell_span.len < 2 {
                continue;
            }
            let child = pass.query(c, query);
            if wants_min {
                let extents = &mut pass.grid_track_aggregator()[min_base..min_base + n_tracks];
                distribute_span(tracks, cell_span, gap, child.min, extents);
            }
            if wants_max {
                let extents = &mut pass.grid_track_aggregator()[max_base..max_base + n_tracks];
                distribute_span(tracks, cell_span, gap, child.max, extents);
            }
        }

        let gaps = gap.gaps_between(n_tracks);
        let mut range = IntrinsicRange::ZERO;
        if wants_min {
            range.min = pass.grid_track_aggregator()[min_base..min_base + n_tracks]
                .iter()
                .sum::<f32>()
                + gaps;
        }
        if wants_max {
            range.max = pass.grid_track_aggregator()[max_base..max_base + n_tracks]
                .iter()
                .sum::<f32>()
                + gaps;
        }
        pass.grid_track_aggregator().truncate(base);
        range
    }
}

/// Grow the Hug tracks `span` crosses until the span holds `need` (CSS Grid §12.5.1, WPF
/// `MeasureCellsGroup`): the shortfall over the tracks' extents and the gaps between them splits
/// evenly over those Hug tracks. A track that reaches its `max` stops there, as the track solve
/// holds it, and what it could not take splits again over the rest. A Fixed track's extent is its
/// size; `extents` holds the rest. A span with no Hug track below its max grows nothing, as a
/// Fixed track cannot and a Fill one takes leftover.
fn distribute_span(tracks: &[Track], span: Span, gap: f32, need: f32, extents: &mut [f32]) {
    let range = span.range();
    let crossed = &tracks[range.clone()];
    let extents = &mut extents[range];
    let held: f32 = crossed
        .iter()
        .zip(extents.iter())
        .map(|(t, &extent)| {
            t.sizing()
                .fixed_value()
                .map_or(extent, |v| v.clamp(t.min(), t.max()))
        })
        .sum::<f32>()
        + gap.gaps_between(crossed.len());
    let mut shortfall = need - held;
    // Each round fills every open track or closes at least one, so one round per track and a last
    // one suffice. Bounded, not `while`: a share too small to move a large extent in f32 would
    // leave the shortfall where it was.
    for _ in 0..=crossed.len() {
        if shortfall <= 0.0 {
            return;
        }
        let open = crossed
            .iter()
            .zip(extents.iter())
            .filter(|(t, extent)| t.sizing().is_hug() && **extent < t.max())
            .count();
        if open == 0 {
            return;
        }
        let share = shortfall / open as f32;
        for (t, extent) in crossed.iter().zip(extents.iter_mut()) {
            if t.sizing().is_hug() && *extent < t.max() {
                let grown = (*extent + share).min(t.max());
                shortfall -= grown - *extent;
                *extent = grown;
            }
        }
    }
}

#[cfg(test)]
mod tests;
