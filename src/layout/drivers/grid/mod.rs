//! Grid driver: the measure / arrange / intrinsic entry points for a `LayoutMode::Grid` node; the track solve
//! and its state are [`GridTrackStore`](grid_track_store::GridTrackStore)'s.

use crate::common::span::Span;
use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::grid::grid_track_store::{GridAxis, GridAxisId};
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
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

mod arrange;
pub(crate) mod grid_ctx;
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

        for flexible in [false, true] {
            for c in tree.active_children(self.node) {
                let cell_span = self.cell(tree, c).track_span(axis);
                if cell_span.len < 2 || crosses_fill(tracks, cell_span) != flexible {
                    continue;
                }
                let child = pass.query(c, query);
                if wants_min {
                    let extents = &mut pass.grid_track_aggregator().since(base)
                        [min_base..min_base + track_count];
                    grid_axis.distribute_span(cell_span, child.min, extents);
                }
                if wants_max {
                    let extents = &mut pass.grid_track_aggregator().since(base)
                        [max_base..max_base + track_count];
                    grid_axis.distribute_span(cell_span, child.max, extents);
                }
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
            range.max = sum_non_fill(tracks, extents) + fill_extent(tracks, extents) + gaps;
        }
        pass.grid_track_aggregator().truncate(base);
        range
    }
}

/// The flexible fraction the Fill tracks of `tracks` need to hold their `extents`, each at least
/// its track's `min`, where no leftover exists to share, as CSS Grid finds the size of an `fr` for
/// indefinite free space (§12.7.1): the largest extent over its track's weight, so a track of
/// weight `w` takes `fraction · w` and holds its extent. `0` with no Fill track.
pub(super) fn fill_fraction(tracks: &[Track], extents: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(extents)
        .filter_map(|(t, &extent)| Some(extent.max(t.min()) / t.sizing().fill_weight()?))
        .fold(0.0, f32::max)
}

/// What the Fill tracks of `tracks` take at the [`fill_fraction`] of `extents`: the fraction times
/// their summed weight.
pub(super) fn fill_extent(tracks: &[Track], extents: &[f32]) -> f32 {
    let weights: f32 = tracks.iter().filter_map(|t| t.sizing().fill_weight()).sum();
    fill_fraction(tracks, extents) * weights
}

/// The sum of `sizes` over the tracks that do not Fill.
pub(super) fn sum_non_fill(tracks: &[Track], sizes: &[f32]) -> f32 {
    sum_of(tracks, sizes, |t| t.sizing().fill_weight().is_none())
}

/// The sum of `sizes` over the tracks that Fill.
pub(super) fn sum_fill(tracks: &[Track], sizes: &[f32]) -> f32 {
    sum_of(tracks, sizes, |t| t.sizing().fill_weight().is_some())
}

fn sum_of(tracks: &[Track], sizes: &[f32], keep: impl Fn(&Track) -> bool) -> f32 {
    tracks
        .iter()
        .zip(sizes)
        .filter(|(t, _)| keep(t))
        .map(|(_, &size)| size)
        .sum()
}

/// Whether `span` crosses a Fill track of `tracks`: such a span grows only its Fill tracks
/// ([`GridAxis::distribute_span`]), and after every span that crosses none, as CSS Grid §12.5 steps 3 and 4
/// order them.
pub(super) fn crosses_fill(tracks: &[Track], span: Span) -> bool {
    has_fill(&tracks[span.range()])
}

pub(super) fn has_fill(tracks: &[Track]) -> bool {
    tracks.iter().any(|t| t.sizing().fill_weight().is_some())
}

impl GridAxis<'_> {
    /// Grow the tracks `span` crosses until the span holds `need` (CSS Grid §12.5 steps 3 and 4):
    /// the shortfall over the tracks' extents and the gaps between them splits over the tracks that
    /// grow. A span that crosses a Fill track grows only its Fill tracks, in proportion to their
    /// weights, and holds every other track at its extent (step 4); one that crosses none grows its
    /// Hug tracks evenly (step 3). A track that reaches its `max` stops there, as the track solve
    /// holds it, and what it could not take splits again over the rest; what no track can take
    /// overflows. A Fixed track's extent is its size; `extents` holds the rest. Fill weights split
    /// as WPF star ratios, so a weight under 1 takes its ratio of the whole shortfall, where CSS
    /// splits only that fraction of it.
    fn distribute_span(self, span: Span, need: f32, extents: &mut [f32]) {
        let Self { tracks, gap, .. } = self;
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
        let flexible = has_fill(crossed);
        // The share a track grows by: its weight among the Fill tracks of a span that crosses one,
        // else 1 for each Hug track; 0 for a track that does not grow.
        let weight = |t: &Track| {
            if flexible {
                t.sizing().fill_weight().unwrap_or(0.0)
            } else if t.sizing().is_hug() {
                1.0
            } else {
                0.0
            }
        };
        let mut shortfall = need - held;
        // Each round fills every open track or closes at least one, so one round per track and a
        // last one suffice. Bounded, not `while`: a share too small to move a large extent in f32
        // would leave the shortfall where it was.
        for _ in 0..=crossed.len() {
            if shortfall <= 0.0 {
                return;
            }
            let open: f32 = crossed
                .iter()
                .zip(extents.iter())
                .filter(|(t, extent)| **extent < t.max())
                .map(|(t, _)| weight(t))
                .sum();
            if open <= 0.0 {
                return;
            }
            let per_weight = shortfall / open;
            for (t, extent) in crossed.iter().zip(extents.iter_mut()) {
                let w = weight(t);
                if w > 0.0 && *extent < t.max() {
                    let grown = (*extent + per_weight * w).min(t.max());
                    shortfall -= grown - *extent;
                    *extent = grown;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
