use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::pass::LayoutPass;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode};
use crate::primitives::interned_text::InternedText;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::record::NodeId;
use std::ops::Range;

mod arranging;
mod axis_scratch;
mod measuring;

use crate::layout::grid::arranging::arrange_inner;
use crate::layout::grid::axis_scratch::{AxisScratch, HugRanges};
use crate::layout::grid::measuring::measure_inner;
#[derive(Clone, Copy, Debug)]
enum HugKind {
    Max,
    Min,
}

/// Pack/unpack order for hug arrays inside a snapshot. Single source of
/// truth — `snapshot_subtree` and `restore_subtree` both iterate this,
/// so reordering one without the other is impossible.
const HUG_ORDER: [(Axis, HugKind); 4] = [
    (Axis::X, HugKind::Max),
    (Axis::X, HugKind::Min),
    (Axis::Y, HugKind::Max),
    (Axis::Y, HugKind::Min),
];

/// Zero this grid's hug arrays so a re-measure of the grid (e.g.,
/// `LayoutEngine::measure`'s grow-driven second pass) starts with a
/// clean accumulator. Both Phase 1 col-intrinsic queries and Phase 2
/// cell-height records merge via `slot[i] = slot[i].max(...)`; without
/// this reset, a re-measure under a wider `available` would keep the
/// previous narrower-pass row heights, leaving cells over-allocated
/// and inflating the grid's `desired.h`. Measure-only — arrange must
/// preserve these. Pinned by
/// `cross_driver_tests::parent_contains_child::two_hug_cols_section_height_matches_post_grow_text`.
fn reset_hugs_for(pass: &mut LayoutPass<'_>, idx: GridDefId) {
    let track_state = pass.grid_track_state_mut();
    for (axis, kind) in HUG_ORDER {
        track_state.slice_mut(idx, axis, kind).fill(0.0);
    }
}

/// Per-frame scratch for `Grid` layout. Capacity is retained across frames; a
/// `Vec<GridScratch>` indexed by nesting depth lets nested grids each have
/// their own slot. Pushed on first descent to a new depth.
#[derive(Debug, Default)]
struct GridScratch {
    col: AxisScratch,
    row: AxisScratch,
}

/// All grid-layout scratch held by `LayoutEngine`, in one bag. `depth_stack`
/// and `track_state` are separate fields so callers can disjoint-borrow them —
/// `AxisScratch::resolve_axis` takes `&mut self` (from `depth_stack`) and `&[f32]`
/// hug slices (from `track_state`) in the same expression via destructuring.
/// `track_aggregator` is a bump-stack scratch for `grid::intrinsic`'s
/// per-track aggregator: each call extends by `n_tracks`, recurses (which
/// may extend further but always truncates back), then truncates to its
/// own base. Capacity retained.
#[derive(Debug, Default)]
pub(crate) struct GridContext {
    pub(crate) depth_stack: GridDepthStack,
    pub(crate) track_state: GridTrackStore,
    pub(super) track_aggregator: Vec<f32>,
}

/// Nesting stack of per-depth grid scratch. One `GridScratch` slot per
/// active `LayoutMode::Grid` ancestor. `depth` is the next free slot.
#[derive(Debug, Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    pub(crate) depth: usize,
}

impl GridDepthStack {
    /// Reserve a scratch slot for the next nesting depth. Grows on first
    /// descent; reuses thereafter.
    fn enter(&mut self) -> usize {
        let d = self.depth;
        if self.scratch.len() == d {
            self.scratch.push(GridScratch::default());
        }
        self.depth = d + 1;
        d
    }

    fn exit(&mut self) {
        debug_assert!(self.depth > 0, "GridDepthStack::exit underflow");
        self.depth -= 1;
    }

    fn at(&mut self, depth: usize) -> &mut GridScratch {
        &mut self.scratch[depth]
    }
}

/// Flat per-track pool with one `(rows, cols)` slot per recorded
/// `GridDef` — every grid's track state for the whole layout pass.
///
/// Three things per track, not just the hug ranges the name used to claim:
/// the content ranges (`max`/`min`, fed by Phase-1 cell intrinsics and
/// Phase-2 cell-height accumulation), the measure-resolved track sizes
/// (`sizes`, the output of [`AxisScratch::resolve_axis`]), and
/// the input `total` each axis was resolved against (`totals`). Measure
/// pass writes; arrange pass reads. Per-depth scratch in `depth_stack` gets
/// clobbered by sibling grids before arrange runs, so the pool persists for
/// the whole layout pass instead.
///
/// `reset_for` zeroes every slot at the top of each pass — load-bearing
/// for `max`/`min`/`sizes` because the Phase 1 column loop and the
/// Phase 2 cell-height accumulator both merge via `slot[i] =
/// slot[i].max(...)` and assume a 0.0 starting state. `totals` resets to
/// `None`, which is how arrange recognises a grid measure never reached
/// this frame (the cache-hit-ancestor path) and re-resolves it.
///
/// Capacity retained across frames.
#[derive(Debug, Default)]
pub(crate) struct GridTrackStore {
    max_pool: Vec<f32>,
    min_pool: Vec<f32>,
    /// Resolved track sizes from the last measure of each grid. Parallel
    /// indexing to `max_pool`/`min_pool` via the same per-slot spans.
    /// Read by arrange to skip a redundant `AxisScratch::resolve_axis` call when the
    /// arrange-time slot matches the measure-time total.
    sizes_pool: Vec<f32>,
    /// `[col_total, row_total]` per grid slot — the `total` each axis
    /// was last resolved against, or `None` where measure hasn't run for
    /// it this frame. Arrange compares against the arrange-time slot
    /// extent and reuses persisted sizes on match.
    ///
    /// `Option` rather than a `0.0` sentinel: a grid arranged into a
    /// zero-extent slot resolves against a legitimate `0.0`, so a
    /// sentinel that spelled "unmeasured" the same way would drop that
    /// grid off the reuse path every frame.
    totals_pool: Vec<[Option<f32>; 2]>,
    slots: Vec<GridTrackSlot>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GridTrackSlot {
    rows: Span,
    cols: Span,
}

impl GridTrackStore {
    pub(crate) fn reset_for(&mut self, tree: &Tree) {
        self.max_pool.clear();
        self.min_pool.clear();
        self.sizes_pool.clear();
        self.totals_pool.clear();
        self.slots.clear();
        for def in &tree.grid_defs {
            let rows = self.alloc(def.rows.len as usize);
            let cols = self.alloc(def.cols.len as usize);
            self.slots.push(GridTrackSlot { rows, cols });
            self.totals_pool.push([None, None]);
        }
    }

    fn alloc(&mut self, n: usize) -> Span {
        let start = self.max_pool.len() as u32;
        self.max_pool.resize(start as usize + n, 0.0);
        self.min_pool.resize(start as usize + n, 0.0);
        self.sizes_pool.resize(start as usize + n, 0.0);
        Span::new(start, n as u32)
    }

    fn axis_slice(&self, idx: GridDefId, axis: Axis) -> Range<usize> {
        let slot = self.slots[usize::from(idx)];
        let s = match axis {
            Axis::X => slot.cols,
            Axis::Y => slot.rows,
        };
        s.range()
    }

    fn slice(&self, idx: GridDefId, axis: Axis, kind: HugKind) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        match kind {
            HugKind::Max => &self.max_pool[r],
            HugKind::Min => &self.min_pool[r],
        }
    }

    fn slice_mut(&mut self, idx: GridDefId, axis: Axis, kind: HugKind) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        match kind {
            HugKind::Max => &mut self.max_pool[r],
            HugKind::Min => &mut self.min_pool[r],
        }
    }

    /// Both pools' slices for one `(idx, axis)` in one call. Single
    /// slot lookup; the borrow checker splits the `&mut self` because
    /// `min_pool` and `max_pool` are separate fields.
    /// Both content-range pools for `(idx, axis)`, as the solver's
    /// input bundle.
    fn ranges(&self, idx: GridDefId, axis: Axis) -> HugRanges<'_> {
        HugRanges {
            min: self.slice(idx, axis, HugKind::Min),
            max: self.slice(idx, axis, HugKind::Max),
        }
    }

    fn slice_mut_pair(&mut self, idx: GridDefId, axis: Axis) -> (&mut [f32], &mut [f32]) {
        let r = self.axis_slice(idx, axis);
        (&mut self.min_pool[r.clone()], &mut self.max_pool[r])
    }

    fn axis_total_idx(axis: Axis) -> usize {
        match axis {
            Axis::X => 0,
            Axis::Y => 1,
        }
    }

    /// Persisted resolved track sizes for `(idx, axis)` from the last
    /// measure. Empty-equivalent until measure writes via
    /// `record_resolution`.
    fn sizes_slice(&self, idx: GridDefId, axis: Axis) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        &self.sizes_pool[r]
    }

    /// `total` (measure-time `AxisScratch::resolve_axis` input) for `(idx, axis)`, or
    /// `None` for grids measure hasn't reached this frame (e.g. cache-hit
    /// descendants); arrange treats that as "no persisted state" and
    /// re-resolves.
    fn total_used(&self, idx: GridDefId, axis: Axis) -> Option<f32> {
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)]
    }

    /// Snapshot the just-resolved `(sizes, total)` for `(idx, axis)`
    /// so a sibling-clobber-resistant arrange can read them back
    /// without re-running `AxisScratch::resolve_axis`. Caller passes the same
    /// `total` it just handed to `AxisScratch::resolve_axis` plus the resolved
    /// `sizes` slice from the per-depth scratch.
    fn record_resolution(&mut self, idx: GridDefId, axis: Axis, total: f32, sizes: &[f32]) {
        let r = self.axis_slice(idx, axis);
        self.sizes_pool[r].copy_from_slice(sizes);
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)] = Some(total);
    }

    /// Pack per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// in `subtree` (pre-order node-index range) into `out`. Used by
    /// the cross-frame measure cache: when a subtree is snapshotted,
    /// arrange's hug state must be saved so a later cache hit at any
    /// ancestor can restore it via [`Self::restore_subtree`]. Order is
    /// dictated by [`HUG_ORDER`] per Grid, in pre-order.
    pub(crate) fn snapshot_subtree(&self, tree: &Tree, subtree: Range<usize>, out: &mut Vec<f32>) {
        let layouts = tree.records.layout();
        for i in subtree {
            let core = layouts[i];
            if let LayoutMode::Grid(idx) = LayoutMode::from(core.meta) {
                for (axis, kind) in HUG_ORDER {
                    out.extend_from_slice(self.slice(idx, axis, kind));
                }
            }
        }
    }

    /// Inverse of `snapshot_subtree`: walks the same pre-order range
    /// and pours four hug arrays per Grid back into the slot at the
    /// current frame's `idx`. `subtree_hash` equality on the cache key
    /// guarantees same Grid count and same `(n_cols, n_rows)` per
    /// Grid in the same order, so the slice and the walk align.
    pub(crate) fn restore_subtree(&mut self, tree: &Tree, subtree: Range<usize>, tracks: &[f32]) {
        let layouts = tree.records.layout();
        let mut pos = 0usize;
        for i in subtree {
            let core = layouts[i];
            if let LayoutMode::Grid(idx) = LayoutMode::from(core.meta) {
                for (axis, kind) in HUG_ORDER {
                    let dst = self.slice_mut(idx, axis, kind);
                    let n = dst.len();
                    dst.copy_from_slice(&tracks[pos..pos + n]);
                    pos += n;
                }
            }
        }
        debug_assert_eq!(
            pos,
            tracks.len(),
            "snapshot hug slice length disagrees with current subtree's grid descendants \
             (cache key let through a structural change?)",
        );
    }
}

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
pub(crate) mod internals {
    use crate::layout::grid::axis_scratch::{AxisScratch, HugRanges};
    use crate::layout::types::track::Track;

    /// Grid's Phase-3 Fill distributor over the same `(weight, floor,
    /// cap)` triples `stack::internals::distribute_fill` takes, with no
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
