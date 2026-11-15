//! Grid driver: the measure / arrange / intrinsic entry points for a `LayoutMode::Grid` node; the track solve
//! and its state are [`GridTrackStore`](grid_track_store::GridTrackStore)'s.

use crate::common::span::Span;
use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::grid::grid_axis::GridAxis;
use crate::layout::drivers::grid::grid_track_store::GridAxisId;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::ContentMeasure;
use crate::layout::pass::LayoutPass;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::layout_mode::GridDefId;
use crate::primitives::math::num::F32Px;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use std::num::NonZeroU16;

mod arrange;
mod grid_axis;
pub(crate) mod grid_ctx;
pub(crate) mod grid_track_store;
mod measure;

/// The two rounds CSS Grid §12.5 grows spanning cells in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpanRound {
    /// Step 3: the spans that cross no Fill track.
    Rigid,
    /// Step 4: the spans that cross a Fill track.
    Flexible,
}

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
        let tracks = |span: Span| {
            NonZeroU16::new(u16::try_from(span.len).unwrap_or(u16::MAX))
                .expect("a grid that places cells has tracks")
        };
        tree.bounds(child)
            .grid
            .clamped_to(tracks(def.rows), tracks(def.cols))
    }

    /// Each child that spans two tracks or more of `grid_axis`, with its span, in the order CSS
    /// Grid §12.5 grows them: [`SpanRound::Rigid`], then [`SpanRound::Flexible`].
    fn spans<'t>(
        self,
        tree: &'t Tree,
        grid_axis: GridAxis<'t>,
    ) -> impl Iterator<Item = (NodeId, Span)> + 't {
        [SpanRound::Rigid, SpanRound::Flexible]
            .into_iter()
            .flat_map(move |round| self.spans_in(tree, grid_axis, round))
    }

    /// Each child of `round` that spans two tracks or more of `grid_axis`, with its span, in
    /// child order. A grid none of whose cells asked for a span walks no child
    /// ([`GridDef::has_spans`](crate::primitives::layout::grid_def::GridDef::has_spans)).
    fn spans_in<'t>(
        self,
        tree: &'t Tree,
        grid_axis: GridAxis<'t>,
        round: SpanRound,
    ) -> impl Iterator<Item = (NodeId, Span)> + 't {
        let axis = grid_axis.id.axis;
        let walks = tree.grid_defs[usize::from(self.def)].has_spans(axis);
        let flexible = round == SpanRound::Flexible;
        walks
            .then(move || {
                tree.active_children(self.node).filter_map(move |c| {
                    let span = self.cell(tree, c).track_span(axis);
                    (span.len >= 2 && grid_axis.crosses_fill(span) == flexible).then_some((c, span))
                })
            })
            .into_iter()
            .flatten()
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
    /// Arranges from its slot and its own subtree alone.
    fn outside_input(self, _tree: &Tree, _layout: &PlaneLayout) -> Option<Size> {
        None
    }

    /// WPF-style grid measure: resolves Fixed tracks, feeds each child the sum of its spanned tracks
    /// (`∞` if any is unresolved, so the child reports intrinsic), then resolves Hug tracks from
    /// span-1 children and the shortfall of spanning ones, which a span crossing a Fill track gives
    /// to its Fill tracks' floors instead ([`GridAxis::distribute_span`]). Fill tracks resolve at
    /// arrange, except columns a non-Hug grid commits at measure. Each grid's tracks live in
    /// `grid.track_state` for the whole pass, so a nested grid cannot overwrite them between measure
    /// and arrange. The solver is documented on
    /// [`GridTrackStore::resolve_axis`](crate::layout::drivers::grid::grid_track_store::GridTrackStore::resolve_axis).
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> ContentMeasure {
        measure::run(self, pass, inner_avail)
    }

    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        arrange::run(self, pass, inner);
    }

    /// Intrinsic size of a Grid: span-1 cells' per-track contributions, then each spanning cell's shortfall
    /// spread over the tracks it grows ([`GridAxis::distribute_span`]), spans crossing no Fill track first, summed across tracks plus gaps, as in
    /// `measure`; `Fixed` clamps to `[min, max]`, `Hug` and `Fill` share a content floor.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        let tree = pass.tree();
        let def = tree.grid_defs[usize::from(self.def)];
        // An empty dimension means no cells, so the grid measures `Size::ZERO` (see `measure::run`) on both axes.
        if def.cols.len == 0 || def.rows.len == 0 {
            return IntrinsicRange::ZERO;
        }
        let axis = query.axis();
        let grid_axis = self.axis(tree, axis);
        let GridAxis { tracks, gap, .. } = grid_axis;
        let track_count = tracks.len();

        let wants_min = query.includes(LenReq::MinContent);
        let wants_max = query.includes(LenReq::MaxContent);
        let base = pass.grid_track_aggregator().mark();
        let min_base = 0;
        let max_base = usize::from(wants_min) * track_count;
        let slot_count = (usize::from(wants_min) + usize::from(wants_max)) * track_count;
        pass.grid_track_aggregator().push_n(slot_count, 0.0);
        for (i, t) in tracks.iter().enumerate() {
            let min = t.min();
            let initial = t
                .sizing()
                .fixed_value()
                .map_or(min, |value| value.clamp(min, t.max()));
            if wants_min {
                pass.grid_track_aggregator().since(base)[min_base + i] = initial;
            }
            if wants_max {
                pass.grid_track_aggregator().since(base)[max_base + i] = initial;
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
                let slot = &mut pass.grid_track_aggregator().since(base)[min_base + track_idx];
                *slot = slot.max(t.content_floor(child.min));
            }
            if wants_max {
                let slot = &mut pass.grid_track_aggregator().since(base)[max_base + track_idx];
                *slot = slot.max(t.content_floor(child.max));
            }
        }

        for (c, cell_span) in self.spans(tree, grid_axis) {
            let child = pass.query(c, query);
            if wants_min {
                let extents =
                    &mut pass.grid_track_aggregator().since(base)[min_base..min_base + track_count];
                grid_axis.distribute_span(cell_span, child.min, extents);
            }
            if wants_max {
                let extents =
                    &mut pass.grid_track_aggregator().since(base)[max_base..max_base + track_count];
                grid_axis.distribute_span(cell_span, child.max, extents);
            }
        }

        let gaps = gap.gaps_between(track_count);
        let mut range = IntrinsicRange::ZERO;
        if wants_min {
            range.min = pass.grid_track_aggregator().since(base)[min_base..min_base + track_count]
                .iter()
                .sum::<f32>()
                + gaps;
        }
        if wants_max {
            let extents =
                &pass.grid_track_aggregator().since(base)[max_base..max_base + track_count];
            range.max = grid_axis.sum_non_fill(extents) + grid_axis.fill_extent(extents) + gaps;
        }
        pass.grid_track_aggregator().truncate(base);
        range
    }
}

#[cfg(test)]
mod tests;
