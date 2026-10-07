use crate::common::span::Span;
use crate::layout::drivers::grid::axis_scratch::{HugRanges, HugRangesMut};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode};
use crate::scene::tree::Tree;
use std::ops::Range;

/// Which content range a hug array holds: preferred extent or min-content floor.
#[derive(Clone, Copy, Debug)]
pub(super) enum HugKind {
    Max,
    Min,
}

/// Pack/unpack order for hug arrays in a snapshot, shared by `snapshot_grid`
/// and `restore_subtree`.
const HUG_ORDER: [(Axis, HugKind); 4] = [
    (Axis::X, HugKind::Max),
    (Axis::X, HugKind::Min),
    (Axis::Y, HugKind::Max),
    (Axis::Y, HugKind::Min),
];

/// Flat per-track pool with one `(rows, cols)` slot per recorded `GridDef`.
///
/// Holds content ranges (`max`/`min`), measure-resolved track `sizes`, and
/// the `total` each axis was resolved against. Measure writes, arrange reads;
/// per-depth scratch is clobbered by sibling grids before arrange runs.
///
/// `reset_for` zeroes every slot each pass: accumulators merge via
/// `slot[i].max(...)` and assume a 0.0 start. `totals` resets to `None`,
/// which tells arrange measure never reached the grid (cache-hit ancestor).
#[derive(Debug, Default)]
pub(crate) struct GridTrackStore {
    max_pool: Vec<f32>,
    min_pool: Vec<f32>,
    /// Resolved track sizes from the last measure, indexed like `max_pool`.
    sizes_pool: Vec<f32>,
    /// `[col_total, row_total]` per slot: the `total` each axis was last
    /// resolved against, or `None` if measure hasn't run this frame.
    ///
    /// `Option` rather than a `0.0` sentinel: a zero-extent slot resolves
    /// against a legitimate `0.0`.
    totals_pool: Vec<[Option<f32>; 2]>,
    slots: Vec<GridTrackSlot>,
}

#[derive(Clone, Copy, Debug)]
struct GridTrackSlot {
    rows: Span,
    cols: Span,
}

impl GridTrackStore {
    /// Asserts measure has not already run for this grid on this layer.
    ///
    /// Accumulators merge with `slot[i].max(...)`, so a second measure under a
    /// wider `available` would keep the narrower pass's row heights.
    /// `total_used` is the witness: set by [`Self::record_resolution`], cleared
    /// by [`Self::reset_for`].
    pub(super) fn debug_assert_unmeasured(&self, idx: GridDefId) {
        debug_assert!(
            self.totals_pool[usize::from(idx)]
                .iter()
                .all(Option::is_none),
            "grid {idx:?} measured twice on one layer; its hug accumulators would carry over",
        );
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

    /// Both content-range pools for `(idx, axis)`.
    pub(super) fn ranges(&self, idx: GridDefId, axis: Axis) -> HugRanges<'_> {
        HugRanges {
            min: self.slice(idx, axis, HugKind::Min),
            max: self.slice(idx, axis, HugKind::Max),
        }
    }

    /// [`Self::ranges`] for measure to write through.
    pub(super) fn ranges_mut(&mut self, idx: GridDefId, axis: Axis) -> HugRangesMut<'_> {
        let r = self.axis_slice(idx, axis);
        HugRangesMut {
            min: &mut self.min_pool[r.clone()],
            max: &mut self.max_pool[r],
        }
    }

    const fn axis_total_idx(axis: Axis) -> usize {
        match axis {
            Axis::X => 0,
            Axis::Y => 1,
        }
    }

    /// Resolved track sizes for `(idx, axis)` from the last measure.
    pub(super) fn sizes_slice(&self, idx: GridDefId, axis: Axis) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        &self.sizes_pool[r]
    }

    /// The `total` measure resolved `(idx, axis)` against, or `None` if measure
    /// hasn't reached the grid this frame; arrange then re-resolves.
    pub(super) fn total_used(&self, idx: GridDefId, axis: Axis) -> Option<f32> {
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)]
    }

    /// Persist the just-resolved `(sizes, total)` for `(idx, axis)` so arrange
    /// can read them back without re-resolving.
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

    /// Append one Grid's four hug arrays to `out`, in [`HUG_ORDER`], so a
    /// measure-cache hit can restore them via [`Self::restore_subtree`].
    pub(crate) fn snapshot_grid(&self, idx: GridDefId, out: &mut Vec<f32>) {
        for (axis, kind) in HUG_ORDER {
            out.extend_from_slice(self.slice(idx, axis, kind));
        }
    }

    /// Inverse of [`Self::snapshot_grid`] over a whole subtree, in pre-order.
    /// The cache key's `subtree_hash` guarantees matching grid count and shape.
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
