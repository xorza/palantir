//! Every grid's per-track state for the whole layout pass, in one flat pool.

use crate::layout::axis::Axis;
use crate::layout::grid::axis_scratch::HugRanges;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode};
use crate::primitives::span::Span;
use crate::scene::tree::Tree;
use std::ops::Range;

/// Which end of a track's content range a hug array holds: its
/// preferred extent, or its min-content floor. The two are stored in
/// separate pools, so this is what picks between them.
#[derive(Clone, Copy, Debug)]
pub(super) enum HugKind {
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
    /// Zero this grid's hug arrays so a re-measure of the grid (e.g.,
    /// `LayoutEngine::measure`'s grow-driven second pass) starts with a
    /// clean accumulator. Both Phase 1 col-intrinsic queries and Phase 2
    /// cell-height records merge via `slot[i] = slot[i].max(...)`; without
    /// this reset, a re-measure under a wider `available` would keep the
    /// previous narrower-pass row heights, leaving cells over-allocated
    /// and inflating the grid's `desired.h`. Measure-only — arrange must
    /// preserve these. Pinned by
    /// `cross_driver_tests::parent_contains_child::two_hug_cols_section_height_matches_post_grow_text`.
    pub(super) fn reset_hugs(&mut self, idx: GridDefId) {
        for (axis, kind) in HUG_ORDER {
            self.slice_mut(idx, axis, kind).fill(0.0);
        }
    }

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

    pub(super) fn alloc(&mut self, n: usize) -> Span {
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

    pub(super) fn slice(&self, idx: GridDefId, axis: Axis, kind: HugKind) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        match kind {
            HugKind::Max => &self.max_pool[r],
            HugKind::Min => &self.min_pool[r],
        }
    }

    pub(super) fn slice_mut(&mut self, idx: GridDefId, axis: Axis, kind: HugKind) -> &mut [f32] {
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
    pub(super) fn ranges(&self, idx: GridDefId, axis: Axis) -> HugRanges<'_> {
        HugRanges {
            min: self.slice(idx, axis, HugKind::Min),
            max: self.slice(idx, axis, HugKind::Max),
        }
    }

    pub(super) fn slice_mut_pair(
        &mut self,
        idx: GridDefId,
        axis: Axis,
    ) -> (&mut [f32], &mut [f32]) {
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
    pub(super) fn sizes_slice(&self, idx: GridDefId, axis: Axis) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        &self.sizes_pool[r]
    }

    /// `total` (measure-time `AxisScratch::resolve_axis` input) for `(idx, axis)`, or
    /// `None` for grids measure hasn't reached this frame (e.g. cache-hit
    /// descendants); arrange treats that as "no persisted state" and
    /// re-resolves.
    pub(super) fn total_used(&self, idx: GridDefId, axis: Axis) -> Option<f32> {
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)]
    }

    /// Snapshot the just-resolved `(sizes, total)` for `(idx, axis)`
    /// so a sibling-clobber-resistant arrange can read them back
    /// without re-running `AxisScratch::resolve_axis`. Caller passes the same
    /// `total` it just handed to `AxisScratch::resolve_axis` plus the resolved
    /// `sizes` slice from the per-depth scratch.
    pub(super) fn record_resolution(
        &mut self,
        idx: GridDefId,
        axis: Axis,
        total: f32,
        sizes: &[f32],
    ) {
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
