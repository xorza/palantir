//! The per-grid track state, and the track-sizing solve that fills it. Every phase both the
//! measure and arrange drivers run on a grid axis is a method here, so both depend down on this
//! file rather than on each other.

use crate::common::span::Span;
use crate::layout::axis_share;
use crate::layout::fill_item::{FillDemand, FillItem};
use crate::layout::hug_item::HugItem;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode};
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;
use crate::scene::tree::Tree;
use std::ops::Range;

/// One axis of one recorded grid: the key [`GridTrackStore`] slices its pools by.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridAxisId {
    pub(super) def: GridDefId,
    pub(super) axis: Axis,
}

/// One grid axis as the solve reads it: which grid and axis, its tracks, and the gap between
/// them.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridAxis<'a> {
    pub(super) id: GridAxisId,
    pub(super) tracks: &'a [Track],
    pub(super) gap: f32,
}

/// The per-track content range one axis solves against: `min[i]` is track `i`'s min-content
/// floor, `max[i]` its preferred extent. Bundled because two adjacent same-typed `&[f32]`
/// parameters swap silently, and the common path would not fail a test.
#[derive(Debug)]
pub(super) struct HugRangesMut<'a> {
    pub(super) min: &'a mut [f32],
    pub(super) max: &'a mut [f32],
}

/// Which content range a hug array holds: preferred extent or min-content floor.
#[derive(Clone, Copy, Debug)]
enum HugKind {
    Max,
    Min,
}

/// Pack/unpack order for hug arrays in a snapshot, shared by `snapshot_grid` and
/// `restore_subtree`.
const HUG_ORDER: [(Axis, HugKind); 4] = [
    (Axis::X, HugKind::Max),
    (Axis::X, HugKind::Min),
    (Axis::Y, HugKind::Max),
    (Axis::Y, HugKind::Min),
];

/// Flat per-track pools with one `(cols, rows)` slot per recorded `GridDef`: the content ranges
/// (`max`/`min`), the resolved `sizes`, which of them are `resolved`, their `offsets`, and the
/// `extents` measure found each Fill track must hold. A grid's tracks are its own for the whole
/// pass, so a nested grid measured between its parent's measure and arrange cannot overwrite
/// them. Refused: a per-depth scratch beside the store, whose sizes measure copied into the store
/// and arrange copied back, with an `enter`/`exit` pairing to keep the depths apart.
///
/// `reset_for` zeroes every slot each pass: accumulators merge via `slot[i].max(...)` and assume
/// a 0.0 start, and a slot's `total` resets to `None`, which tells arrange measure never reached
/// the grid (cache-hit ancestor).
#[derive(Debug, Default)]
pub(crate) struct GridTrackStore {
    max_pool: Vec<f32>,
    min_pool: Vec<f32>,
    sizes_pool: Vec<f32>,
    /// Whether a track's size is known at measure: Fixed and Hug ones are, Fill ones only once a
    /// grid that does not hug commits them ([`Self::commit_fill`]).
    resolved_pool: Vec<bool>,
    offsets_pool: Vec<f32>,
    /// Per track, what measure found it must hold: a Fill track its single-track cells' measured
    /// extent, grown by the spans crossing it; any other track its resolved size.
    /// [`grid::fill_fraction`](super::fill_fraction) reads it.
    extents_pool: Vec<f32>,
    /// Per grid, its columns then its rows.
    slots: Vec<[AxisSlot; 2]>,
    /// The solve's transient lists, kept to retain capacity across frames; one for every grid,
    /// as a solve recurses into none.
    hugs: Vec<HugItem<usize>>,
    flexible: Vec<FillItem<usize>>,
}

/// One axis of one grid: its tracks in the pools, and what the last solve of it read and found.
#[derive(Clone, Copy, Debug)]
struct AxisSlot {
    tracks: Span,
    /// The `total` the axis was last resolved against, or `None` if no solve ran this pass.
    /// `Option` rather than a `0.0` sentinel: a zero-extent slot resolves against a legitimate
    /// `0.0`.
    total: Option<f32>,
    /// The least finite `total` from which the last solve sizes its Fixed and Hug tracks the same
    /// (see [`Measured::stable_from`](crate::layout::measured::Measured)). Fixed tracks read no
    /// total; Hug tracks read it only while they do not all fit at their preferred extent. Fill
    /// tracks read every total, which the caller answers for.
    stable_from: f32,
}

const fn axis_idx(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
    }
}

impl GridTrackStore {
    /// Asserts measure has not already run for this grid on this layer.
    ///
    /// Accumulators merge with `slot[i].max(...)`, so a second measure under a wider `available`
    /// would keep the narrower pass's row heights. A slot's `total` is the witness: set by
    /// [`Self::resolve_axis`], cleared by [`Self::reset_for`].
    pub(super) fn debug_assert_unmeasured(&self, def: GridDefId) {
        debug_assert!(
            self.slots[usize::from(def)]
                .iter()
                .all(|slot| slot.total.is_none()),
            "grid {def:?} measured twice on one plane; its hug accumulators would carry over",
        );
    }

    pub(crate) fn reset_for(&mut self, tree: &Tree) {
        self.max_pool.clear();
        self.min_pool.clear();
        self.sizes_pool.clear();
        self.resolved_pool.clear();
        self.offsets_pool.clear();
        self.extents_pool.clear();
        self.slots.clear();
        for def in &tree.grid_defs {
            let cols = self.alloc(def.cols.len);
            let rows = self.alloc(def.rows.len);
            self.slots.push([cols, rows]);
        }
    }

    fn alloc(&mut self, count: u32) -> AxisSlot {
        let start = self.max_pool.len();
        let end = start + count as usize;
        self.max_pool.resize(end, 0.0);
        self.min_pool.resize(end, 0.0);
        self.sizes_pool.resize(end, 0.0);
        self.resolved_pool.resize(end, false);
        self.offsets_pool.resize(end, 0.0);
        self.extents_pool.resize(end, 0.0);
        AxisSlot {
            tracks: Span::new(start as u32, count),
            total: None,
            stable_from: 0.0,
        }
    }

    fn slot(&self, id: GridAxisId) -> &AxisSlot {
        &self.slots[usize::from(id.def)][axis_idx(id.axis)]
    }

    fn slot_mut(&mut self, id: GridAxisId) -> &mut AxisSlot {
        &mut self.slots[usize::from(id.def)][axis_idx(id.axis)]
    }

    fn range(&self, id: GridAxisId) -> Range<usize> {
        self.slot(id).tracks.range()
    }

    fn hug(&self, id: GridAxisId, kind: HugKind) -> &[f32] {
        let r = self.range(id);
        match kind {
            HugKind::Max => &self.max_pool[r],
            HugKind::Min => &self.min_pool[r],
        }
    }

    fn hug_mut(&mut self, id: GridAxisId, kind: HugKind) -> &mut [f32] {
        let r = self.range(id);
        match kind {
            HugKind::Max => &mut self.max_pool[r],
            HugKind::Min => &mut self.min_pool[r],
        }
    }

    /// The min-content floor of each track of `id`.
    pub(super) fn content_min(&self, id: GridAxisId) -> &[f32] {
        self.hug(id, HugKind::Min)
    }

    /// Both content-range pools of `id`, for measure to write through.
    pub(super) fn ranges_mut(&mut self, id: GridAxisId) -> HugRangesMut<'_> {
        let r = self.range(id);
        HugRangesMut {
            min: &mut self.min_pool[r.clone()],
            max: &mut self.max_pool[r],
        }
    }

    /// The resolved size of each track of `id`.
    pub(super) fn sizes(&self, id: GridAxisId) -> &[f32] {
        &self.sizes_pool[self.range(id)]
    }

    /// What each track of `id` must hold, as measure found it.
    pub(super) fn extents(&self, id: GridAxisId) -> &[f32] {
        &self.extents_pool[self.range(id)]
    }

    pub(super) fn extents_mut(&mut self, id: GridAxisId) -> &mut [f32] {
        let r = self.range(id);
        &mut self.extents_pool[r]
    }

    /// Where track `track` of `id` starts, after [`Self::compute_offsets`].
    pub(super) fn offset(&self, id: GridAxisId, track: usize) -> f32 {
        self.offsets_pool[self.range(id)][track]
    }

    /// What the last solve of `id` holds from ([`AxisSlot::stable_from`]).
    pub(super) fn stable_from(&self, id: GridAxisId) -> f32 {
        self.slot(id).stable_from
    }

    /// Sum of spanned tracks' resolved sizes, or `∞` if any is not yet resolved (Hug / Fill at
    /// measure time); gaps count only when the whole span is known. Infinity makes the child fall
    /// back to its intrinsic size (the WPF trick).
    pub(super) fn known_span_size(&self, id: GridAxisId, span: Span, gap: f32) -> f32 {
        let resolved = &self.resolved_pool[self.range(id)];
        if !resolved[span.range()].iter().all(|&known| known) {
            return f32::INFINITY;
        }
        self.span_size(id, span, gap)
    }

    /// Sum of the spanned tracks' sizes plus gaps between them: what arrange gives a cell, and
    /// the resolved half of [`Self::known_span_size`]. Indexes directly: the driver coerces every
    /// cell into the tracks first (`Grid::cell`).
    pub(super) fn span_size(&self, id: GridAxisId, span: Span, gap: f32) -> f32 {
        self.sizes(id)[span.range()].iter().sum::<f32>() + gap.gaps_between(span.len as usize)
    }

    /// Cumulative offset of each track of `id` from the axis origin, gaps included.
    pub(super) fn compute_offsets(&mut self, id: GridAxisId, gap: f32) {
        let r = self.range(id);
        let count = r.len();
        let mut acc = 0.0f32;
        for (i, (offset, &size)) in self.offsets_pool[r.clone()]
            .iter_mut()
            .zip(&self.sizes_pool[r])
            .enumerate()
        {
            *offset = acc;
            acc += size;
            if i + 1 < count {
                acc += gap;
            }
        }
    }

    /// Keep the sizes the last solve found, or solve again, whichever is sound for arrange's slot
    /// `total` on `axis`. Arrange reads only sizes and offsets, so Fill stays uncommitted.
    pub(super) fn resolve_or_reuse(&mut self, axis: GridAxis<'_>, total: f32) {
        // `Some(total)` covers both conditions: `None` means measure never ran, any other extent
        // means the slot moved since. An infinite measure-time total (a Hug grid) never equals
        // arrange's finite slot, so it re-resolves.
        if self.slot(axis.id).total != Some(total) {
            self.resolve_axis(axis, total);
        }
    }

    /// Phase 1 of [`Self::resolve_axis`], also run standalone by grid measure before the
    /// per-cell loop so `known_span_size` reads Fixed rows as resolved while Hug and Fill are
    /// unknown. Returns the extent the Fixed tracks consumed.
    pub(super) fn resolve_fixed(&mut self, id: GridAxisId, tracks: &[Track]) -> f32 {
        let r = self.range(id);
        let sizes = &mut self.sizes_pool[r.clone()];
        let resolved = &mut self.resolved_pool[r];
        let mut consumed = 0.0;
        for (i, t) in tracks.iter().enumerate() {
            if let Some(value) = t.sizing().fixed_value() {
                sizes[i] = value.clamp(t.min(), t.max());
                resolved[i] = true;
                consumed += sizes[i];
            }
        }
        consumed
    }

    /// Resolve `axis`'s track sizes for a grid with `total` available length, and note `total`
    /// as the one they were resolved against.
    ///
    /// Three phases, and a fourth [`Self::commit_fill`] adds:
    /// 1. **Fixed:** clamp `Sizing::fixed(v)` to `[Track.min, Track.max]` and consume from
    ///    available.
    /// 2. **Hug:** solve each track's content range, min-content floor and preferred size both
    ///    capped by `Track.max`, against what Fixed and the Fill tracks' floors leave (as CSS
    ///    Grid, so a Hug sibling never squeezes a Fill column below its content):
    ///    - `sum_hug_max <= remaining`: each Hug at max.
    ///    - `sum_hug_min >= remaining`: each Hug at min, grid overflows.
    ///    - else each starts at min, slack split in proportion to `(max - min)`.
    /// 3. **Fill:** [`FillItem::distribute`] shares the leftover by weight, each clamped to its
    ///    capped floor and `Track.max`.
    pub(super) fn resolve_axis(&mut self, axis: GridAxis<'_>, total: f32) {
        let GridAxis { id, tracks, gap } = axis;
        let r = self.range(id);
        self.sizes_pool[r.clone()].fill(0.0);
        // Fixed and Hug get marked resolved as computed. Fill stays unresolved so cells in Fill
        // columns see INF via `known_span_size`; otherwise they would measure with a finite
        // measure-time leftover and arrange might assign a different slot to a Hug grid,
        // disagreeing with the cell rect.
        self.resolved_pool[r.clone()].fill(false);
        let consumed = gap.gaps_between(tracks.len()) + self.resolve_fixed(id, tracks);

        // Phases 2 and 3: Hug tracks share what Fixed leaves after the Fill floors, and Fill
        // divides the rest (see `axis_share`). Capping the floor at `Track.max` keeps each
        // interval ordered.
        let Self {
            max_pool,
            min_pool,
            sizes_pool,
            resolved_pool,
            hugs,
            flexible,
            ..
        } = self;
        let (min, max) = (&min_pool[r.clone()], &max_pool[r.clone()]);
        hugs.clear();
        flexible.clear();
        for (i, t) in tracks.iter().enumerate() {
            if t.sizing().is_hug() {
                let lo = t.content_floor(min[i]);
                let hi = max[i].max(lo).min(t.max());
                hugs.push(HugItem::new(i, lo, hi));
            } else if let Some(weight) = t.sizing().fill_weight() {
                flexible.push(FillItem::new(
                    i,
                    FillDemand {
                        weight,
                        floor: t.content_floor(min[i]),
                        cap: t.max(),
                    },
                ));
            }
        }
        let budget = (total - consumed).max(0.0);
        let shares_from = axis_share::solve(hugs, flexible, budget);
        let sizes = &mut sizes_pool[r.clone()];
        let resolved = &mut resolved_pool[r];
        for item in hugs.iter() {
            sizes[item.key] = item.size;
            resolved[item.key] = true;
        }
        for item in flexible.iter() {
            sizes[item.key] = item.size;
        }
        let slot = self.slot_mut(id);
        slot.total = Some(total);
        // The shares hold while what Fixed leaves still holds them.
        slot.stable_from = if shares_from > 0.0 {
            consumed + shares_from
        } else {
            0.0
        };
    }

    /// Set what each track of `id` that does not Fill holds to its resolved size.
    pub(super) fn hold_resolved_extents(&mut self, id: GridAxisId, tracks: &[Track]) {
        let r = self.range(id);
        for ((extent, &size), t) in self.extents_pool[r.clone()]
            .iter_mut()
            .zip(&self.sizes_pool[r])
            .zip(tracks)
        {
            if t.sizing().fill_weight().is_none() {
                *extent = size;
            }
        }
    }

    /// Phase 4 of [`Self::resolve_axis`], **Commit:** Fill tracks stay unresolved by default, so
    /// cells in Fill columns see `INF` during measure and Fill is finalized at arrange. When the
    /// grid is non-Hug on this axis with a finite slot, measure's `total` matches arrange's, so
    /// Fill can commit up front and wrap text shapes correctly. Hug grids keep Fill unresolved
    /// (their arrange slot is unknown).
    pub(super) fn commit_fill(&mut self, id: GridAxisId, tracks: &[Track]) {
        let r = self.range(id);
        for (resolved, t) in self.resolved_pool[r].iter_mut().zip(tracks) {
            if t.sizing().fill_weight().is_some() {
                *resolved = true;
            }
        }
    }

    /// Append one Grid's four hug arrays to `out`, in [`HUG_ORDER`], so a measure-cache hit can
    /// restore them via [`Self::restore_subtree`].
    pub(crate) fn snapshot_grid(&self, def: GridDefId, out: &mut Vec<f32>) {
        for (axis, kind) in HUG_ORDER {
            out.extend_from_slice(self.hug(GridAxisId { def, axis }, kind));
        }
    }

    /// Inverse of [`Self::snapshot_grid`] over a whole subtree, in pre-order. The cache key's
    /// `subtree_hash` guarantees matching grid count and shape.
    pub(crate) fn restore_subtree(&mut self, tree: &Tree, subtree: Range<usize>, tracks: &[f32]) {
        let layouts = tree.records.layout();
        let mut pos = 0usize;
        for i in subtree {
            let core = layouts[i];
            if let LayoutMode::Grid(def) = LayoutMode::from(core.meta) {
                for (axis, kind) in HUG_ORDER {
                    let dst = self.hug_mut(GridAxisId { def, axis }, kind);
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

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::layout::drivers::grid::grid_track_store::GridTrackStore;

    impl GridTrackStore {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                max_pool,
                min_pool,
                sizes_pool,
                resolved_pool,
                offsets_pool,
                extents_pool,
                slots,
                hugs,
                flexible,
            } = self;
            scratch_report::vec_bytes(max_pool)
                + scratch_report::vec_bytes(min_pool)
                + scratch_report::vec_bytes(sizes_pool)
                + scratch_report::vec_bytes(resolved_pool)
                + scratch_report::vec_bytes(offsets_pool)
                + scratch_report::vec_bytes(extents_pool)
                + scratch_report::vec_bytes(slots)
                + scratch_report::vec_bytes(hugs)
                + scratch_report::vec_bytes(flexible)
        }
    }
}
