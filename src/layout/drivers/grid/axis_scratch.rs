//! One grid axis's per-depth scratch and the track-sizing solve that fills it.
//! Every phase both the measure and arrange drivers run on a grid axis is a
//! method here, so both depend down on this file rather than on each other.

use crate::common::span::Span;
use crate::layout::axis_share;
use crate::layout::drivers::grid::grid_track_store::{GridAxisId, GridTrackStore};
use crate::layout::fill_item::{FillDemand, FillItem};
use crate::layout::hug_item::HugItem;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;
use fixedbitset::FixedBitSet;

/// Per-axis scratch for one nesting depth. `flexible` and `hugs` are
/// transient lists used inside [`Self::resolve_axis`], kept here to retain
/// capacity across frames. Per-track Hug `[min, max]` ranges live in
/// `GridTrackStore` and are passed to [`Self::resolve_axis`] as slices.
#[derive(Debug, Default)]
pub(super) struct AxisScratch {
    pub(super) sizes: Vec<f32>,
    pub(super) resolved: FixedBitSet,
    pub(super) offsets: Vec<f32>,
    /// The least finite `total` from which the last [`Self::resolve_axis`]
    /// sizes its Fixed and Hug tracks the same (see
    /// [`Measured::stable_from`](crate::layout::measured::Measured)). Fixed
    /// tracks read no total; Hug tracks read it only while they do not all fit
    /// at their preferred extent. Fill tracks read every total, which the
    /// caller answers for.
    pub(super) stable_from: f32,
    /// What each Fill track's single-track cells measured at on this axis, read where the grid
    /// hugs or its offer is indefinite: there a Fill track takes its max-content contribution,
    /// as a Stack's Fill child does.
    pub(super) fill_content: Vec<f32>,
    flexible: Vec<FillItem<usize>>,
    hugs: Vec<HugItem<usize>>,
}

/// One grid axis as the solve reads it: which grid and axis, its tracks, and
/// the gap between them.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridAxis<'a> {
    pub(super) id: GridAxisId,
    pub(super) tracks: &'a [Track],
    pub(super) gap: f32,
}

/// The per-track content range one axis solves against: `min[i]` is track
/// `i`'s min-content floor, `max[i]` its preferred extent. Bundled because
/// two adjacent same-typed `&[f32]` parameters swap silently, and the common
/// path would not fail a test.
#[derive(Clone, Copy, Debug)]
pub(super) struct HugRanges<'a> {
    pub(super) min: &'a [f32],
    pub(super) max: &'a [f32],
}

/// [`HugRanges`] as the measure pass writes it, mutable and bundled for the
/// same reason.
#[derive(Debug)]
pub(super) struct HugRangesMut<'a> {
    pub(super) min: &'a mut [f32],
    pub(super) max: &'a mut [f32],
}

impl AxisScratch {
    /// Resize the per-track arrays, zeroed, and reset `resolved` to all-false.
    /// Capacity is retained.
    pub(super) fn reset_for(&mut self, n: usize) {
        self.sizes.clear();
        self.sizes.resize(n, 0.0);
        self.resolved.clear();
        self.resolved.grow(n);
        self.offsets.clear();
        self.offsets.resize(n, 0.0);
        self.fill_content.clear();
        self.fill_content.resize(n, 0.0);
    }

    /// Sum of spanned tracks' resolved sizes, or `∞` if any is not yet resolved
    /// (Hug / Fill at measure time); gaps count only when the whole span is
    /// known. Infinity makes the child fall back to its intrinsic size (the WPF
    /// trick).
    pub(super) fn known_span_size(&self, span: Span, gap: f32) -> f32 {
        if span.range().any(|i| !self.resolved.contains(i)) {
            return f32::INFINITY;
        }
        self.span_size(span, gap)
    }

    /// Sum of the spanned tracks' sizes plus gaps between them: what arrange
    /// gives a cell, and the resolved half of [`Self::known_span_size`].
    /// Indexes directly: the driver coerces every cell into the tracks first
    /// (`Grid::cell`).
    pub(super) fn span_size(&self, span: Span, gap: f32) -> f32 {
        self.sizes[span.range()].iter().sum::<f32>() + gap.gaps_between(span.len as usize)
    }

    /// Cumulative offset of each track from the axis origin, gaps included.
    pub(super) fn compute_offsets(&mut self, gap: f32) {
        debug_assert_eq!(self.sizes.len(), self.offsets.len());
        let mut acc = 0.0f32;
        for (i, &size) in self.sizes.iter().enumerate() {
            self.offsets[i] = acc;
            acc += size;
            if i + 1 < self.sizes.len() {
                acc += gap;
            }
        }
    }

    /// Either copy persisted resolved sizes from the last measure or re-run
    /// [`Self::resolve_axis`], whichever is sound for arrange's slot `total` on
    /// `axis`. Arrange reads only sizes and offsets, so Fill stays uncommitted.
    pub(super) fn resolve_or_reuse(
        &mut self,
        axis: GridAxis<'_>,
        track_state: &GridTrackStore,
        total: f32,
    ) {
        // `Some(total)` covers both conditions: `None` means measure never ran,
        // any other extent means the slot moved since. An infinite measure-time
        // total (a Hug grid) never equals arrange's finite slot, so it re-resolves.
        if track_state.total_used(axis.id) == Some(total) {
            self.sizes.copy_from_slice(track_state.sizes_slice(axis.id));
            return;
        }
        self.resolve_axis(axis, track_state.ranges(axis.id), total);
    }

    /// Phase 1 of [`Self::resolve_axis`], also run standalone by
    /// grid measure before the per-cell loop so `known_span_size` reads
    /// Fixed rows as resolved while Hug and Fill are unknown. Returns the
    /// extent the Fixed tracks consumed. Callers reset first.
    pub(super) fn resolve_fixed(&mut self, tracks: &[Track]) -> f32 {
        let mut consumed = 0.0;
        for (i, t) in tracks.iter().enumerate() {
            if let Some(value) = t.size.fixed_value() {
                self.sizes[i] = value.clamp(t.min, t.max);
                self.resolved.insert(i);
                consumed += self.sizes[i];
            }
        }
        consumed
    }

    /// Resolve `axis`'s track sizes into [`Self::sizes`] for a grid with
    /// `total` available length.
    ///
    /// Three phases, and a fourth [`Self::commit_fill`] adds:
    /// 1. **Fixed:** clamp `Sizing::fixed(v)` to `[Track.min, Track.max]` and
    ///    consume from available.
    /// 2. **Hug:** solve each track's content range, min-content floor and
    ///    preferred size both capped by `Track.max`, against what Fixed and
    ///    the Fill tracks' floors leave (as CSS Grid, so a Hug sibling never
    ///    squeezes a Fill column below its content):
    ///    - `sum_hug_max <= remaining`: each Hug at max.
    ///    - `sum_hug_min >= remaining`: each Hug at min, grid overflows.
    ///    - else each starts at min, slack split in proportion to `(max - min)`.
    /// 3. **Fill:** [`FillItem::distribute`] shares the leftover by weight,
    ///    each clamped to its capped floor and `Track.max`.
    pub(super) fn resolve_axis(&mut self, axis: GridAxis<'_>, hugs: HugRanges<'_>, total: f32) {
        let GridAxis { tracks, gap, .. } = axis;
        let n = tracks.len();
        self.sizes.fill(0.0);
        // Fixed and Hug get marked resolved as computed. Fill stays unresolved
        // so cells in Fill columns see INF via `known_span_size`; otherwise they
        // would measure with a finite measure-time leftover and arrange might
        // assign a different slot to a Hug grid, disagreeing with the cell rect.
        self.resolved.clear();
        let total_gap = gap.gaps_between(n);

        let consumed = total_gap + self.resolve_fixed(tracks);

        // Phases 2 and 3: Hug tracks share what Fixed leaves after the Fill
        // floors, and Fill divides the rest (see `axis_share`). Capping the
        // floor at `Track.max` keeps each interval ordered.
        self.hugs.clear();
        self.flexible.clear();
        for (i, t) in tracks.iter().enumerate() {
            if t.size.is_hug() {
                let lo = t.content_floor(hugs.min[i]);
                let hi = hugs.max[i].max(lo).min(t.max);
                self.hugs.push(HugItem::new(i, lo, hi));
            } else if let Some(weight) = t.size.fill_weight() {
                self.flexible.push(FillItem::new(
                    i,
                    FillDemand {
                        weight,
                        floor: t.content_floor(hugs.min[i]),
                        cap: t.max,
                    },
                ));
            }
        }
        let budget = (total - consumed).max(0.0);
        let shares_from = axis_share::solve(&mut self.hugs, &mut self.flexible, budget);
        // The shares hold while what Fixed leaves still holds them.
        self.stable_from = if shares_from > 0.0 {
            consumed + shares_from
        } else {
            0.0
        };
        for item in &self.hugs {
            self.sizes[item.key] = item.size;
            self.resolved.insert(item.key);
        }
        for item in &self.flexible {
            self.sizes[item.key] = item.size;
        }
    }

    /// Phase 4 of [`Self::resolve_axis`], **Commit:** Fill tracks stay
    /// unresolved by default, so cells in Fill columns see `INF` during measure
    /// and Fill is finalized at arrange. When the grid is non-Hug on this axis
    /// with a finite slot, measure's `total` matches arrange's, so Fill can
    /// commit up front and wrap text shapes correctly. Hug grids keep Fill
    /// unresolved (their arrange slot is unknown).
    pub(super) fn commit_fill(&mut self, tracks: &[Track]) {
        for (i, t) in tracks.iter().enumerate() {
            if t.size.fill_weight().is_some() {
                self.resolved.insert(i);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::layout::drivers::grid::axis_scratch::AxisScratch;

    impl AxisScratch {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                sizes,
                resolved,
                offsets,
                stable_from: _,
                fill_content,
                flexible,
                hugs,
            } = self;
            // `FixedBitSet` exposes no capacity; its live blocks stand in.
            scratch_report::vec_bytes(sizes)
                + scratch_report::vec_bytes(fill_content)
                + size_of_val(resolved.as_slice())
                + scratch_report::vec_bytes(offsets)
                + scratch_report::vec_bytes(flexible)
                + scratch_report::vec_bytes(hugs)
        }
    }
}
