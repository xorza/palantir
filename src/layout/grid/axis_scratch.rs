//! One grid axis's per-depth scratch, and the track-sizing solve that
//! fills it.
//!
//! Every phase both the measure and arrange drivers run against a grid
//! axis is a method here, so neither driver file has to reach into the
//! other: `measuring` and `arranging` both depend *down* on this one.

use crate::layout::axis::Axis;
use crate::layout::fill_item::FillItem;
use crate::layout::grid::grid_track_store::GridTrackStore;
use crate::layout::types::layout_mode::GridDefId;
use crate::layout::types::track::Track;
use crate::primitives::span::Span;
use fixedbitset::FixedBitSet;

/// Per-axis scratch for one nesting depth. `flexible` and `hug_bounds`
/// are transient lists used only inside [`Self::resolve_axis`]; they live on
/// the per-axis struct so their capacity is retained across frames.
///
/// Per-track content-driven `[min, max]` Hug ranges live in
/// `GridTrackStore` (durable across the whole layout pass); they're passed
/// into [`Self::resolve_axis`] as slices alongside this scratch.
#[derive(Debug, Default)]
pub(super) struct AxisScratch {
    pub(super) sizes: Vec<f32>,
    pub(super) resolved: FixedBitSet,
    pub(super) offsets: Vec<f32>,
    flexible: Vec<FillItem<usize>>,
    hug_bounds: Vec<HugBound>,
}

/// The per-track content range one axis solves against: `min[i]` is
/// track `i`'s min-content floor, `max[i]` its preferred extent.
///
/// Bundled because they were two adjacent same-typed `&[f32]` parameters on
/// [`AxisScratch::resolve_axis`] — swapping them compiles, and
/// the common path (every Hug track fits at its max) wouldn't even fail a
/// test.
#[derive(Clone, Copy, Debug)]
pub(super) struct HugRanges<'a> {
    pub(super) min: &'a [f32],
    pub(super) max: &'a [f32],
}

/// [`HugRanges`] as the measure pass writes it — same two pools for one
/// `(idx, axis)`, mutable, and named for the same reason: two adjacent
/// `&mut [f32]`s swap silently.
#[derive(Debug)]
pub(super) struct HugRangesMut<'a> {
    pub(super) min: &'a mut [f32],
    pub(super) max: &'a mut [f32],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HugBound {
    idx: usize,
    lo: f32,
    hi: f32,
}

impl AxisScratch {
    /// Resize the per-track arrays. All arrays are zeroed; `resolved` is
    /// reset to all-false. Capacity is retained across frames.
    pub(super) fn reset(&mut self, n: usize) {
        self.sizes.clear();
        self.sizes.resize(n, 0.0);
        self.resolved.clear();
        self.resolved.grow(n);
        self.offsets.clear();
        self.offsets.resize(n, 0.0);
    }

    /// Sum of spanned tracks' resolved sizes, or `∞` if any spanned track is not
    /// yet resolved (Hug / Fill at measure time). Internal gaps contribute only
    /// when the whole span is known. Infinity makes the child fall back to its
    /// intrinsic size on that axis (the WPF trick).
    pub(super) fn known_span_size(&self, span: Span, gap: f32) -> f32 {
        // Cells are range-checked against the parent's track counts at record
        // time (`Tree::check_grid_cell`), so `span.range()` is always in
        // bounds here — index directly.
        let mut sum = 0.0;
        for i in span.range() {
            if !self.resolved.contains(i) {
                return f32::INFINITY;
            }
            sum += self.sizes[i];
        }
        sum + gap * span.len.saturating_sub(1) as f32
    }

    /// Either copy persisted resolved sizes from the last measure or
    /// re-run [`Self::resolve_axis`] — whichever is sound for arrange's
    /// `(grid, axis, slot)`. See the call-site comment for the
    /// soundness conditions; the predicate here is just the boolean
    /// version of those.
    pub(super) fn resolve_or_reuse(
        &mut self,
        tracks: &[Track],
        track_state: &mut GridTrackStore,
        idx: GridDefId,
        axis: Axis,
        total: f32,
        gap: f32,
    ) {
        // `Some(total)` covers both conditions at once: a `None` slot means
        // measure never ran for this grid, and any other recorded extent
        // means the slot moved since it did. An infinite measure-time total
        // (a Hug grid) never equals arrange's finite slot, so it falls
        // through to the re-resolve like any other mismatch.
        if track_state.total_used(idx, axis) == Some(total) {
            self.sizes
                .copy_from_slice(track_state.sizes_slice(idx, axis));
            return;
        }
        self.resolve_axis(tracks, track_state.ranges(idx, axis), total, gap, false);
    }

    /// Phase 1 of [`Self::resolve_axis`], also run standalone by
    /// `measure_inner`
    /// before the per-cell loop so `known_span_size` reads Fixed rows as
    /// resolved while Hug and Fill rows are still unknown. Returns the total
    /// extent the Fixed tracks consumed, which is what `resolve_axis` needs
    /// and the standalone caller ignores. Callers reset `a` first — both do,
    /// via [`Self::reset`] or [`Self::resolve_axis`]'s own `fill`/`clear`.
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

    /// Resolve track sizes on one axis into `a.sizes` for a grid with
    /// `total` available main-axis length and `gap` between adjacent tracks.
    /// `commit_fill` marks Fill tracks resolved when measure knows its
    /// available extent is the final arrange extent.
    ///
    /// **Algorithm**, four phases:
    /// 1. **Fixed:** clamp `Sizing::fixed(v)` to `[Track.min, Track.max]`,
    ///    consume from available.
    /// 2. **Hug:** constraint-solve each track's content range, with both
    ///    its min-content floor and preferred size capped by `Track.max`,
    ///    against the remaining-after-Fixed:
    ///    - If `sum_hug_max <= remaining`: each Hug at max.
    ///    - If `sum_hug_min >= remaining`: each Hug at min, grid overflows.
    ///    - Else: each Hug starts at min, slack distributed proportional to
    ///      `(max - min)`.
    /// 3. **Fill:** [`FillItem::distribute`] — Fill tracks share the
    ///    leftover proportional to weight, each clamped to its capped
    ///    min-content floor and `Track.max`.
    /// 4. **Mark Fill resolved (commit):** by default Fill tracks stay
    ///    unresolved so cells in Fill cols see `INF` via `known_span_size`
    ///    during measure (preserves "Fill is finalized at arrange"). When
    ///    the grid itself is non-Hug on this axis with a finite slot, the
    ///    measure-time `total` matches arrange's, so Fill tracks can be
    ///    committed up-front and cells measure at the resolved width — wrap
    ///    text shapes correctly. Hug grids must keep Fill unresolved (their
    ///    arrange slot is unknown here). Arrange passes `false` because it
    ///    consumes only sizes and offsets, never the resolved flags.
    pub(super) fn resolve_axis(
        &mut self,
        tracks: &[Track],
        hugs: HugRanges<'_>,
        total: f32,
        gap: f32,
        commit_fill: bool,
    ) {
        let n = tracks.len();
        self.sizes.fill(0.0);
        // Reset resolved flags. Fixed + Hug get marked resolved as they're
        // computed. Fill stays unresolved so cells in Fill cols see INF as
        // their available width via `known_span_size`, which is what makes
        // "Fill is finalized at arrange" hold. Without this, cells in
        // Fill cols would measure with measure-time Fill leftover (a
        // finite value), then arrange might assign a different
        // intrinsic-floor-driven slot to a Hug grid and the cell
        // rect/shape would disagree.
        self.resolved.clear();
        let total_gap = gap * n.saturating_sub(1) as f32;

        // Phase 1: Fixed.
        let mut consumed = total_gap + self.resolve_fixed(tracks);

        // Phase 2: Hug, constraint-solved against remaining-after-Fixed.
        // Single pass: snapshot each Hug track's clamped `(lo, hi)` once,
        // pick the distribution rule from the totals, then write sizes.
        self.hug_bounds.clear();
        let mut hug_min_sum = 0.0_f32;
        let mut hug_max_sum = 0.0_f32;
        for (i, t) in tracks.iter().enumerate() {
            if t.size.is_hug() {
                let lo = t.content_floor(hugs.min[i]);
                let hi = hugs.max[i].max(lo).min(t.max);
                hug_min_sum += lo;
                hug_max_sum += hi;
                self.hug_bounds.push(HugBound { idx: i, lo, hi });
            }
        }

        if !self.hug_bounds.is_empty() {
            let remaining_after_fixed = (total - consumed).max(0.0);
            // Pick distribution mode once. `unconstrained` covers infinite
            // total (Hug parent) and the "every Hug fits at max" case;
            // `cramped` covers "even at min the Hugs overflow"; otherwise
            // distribute slack proportional to per-track `(hi - lo)`.
            let unconstrained = total.is_infinite() || hug_max_sum <= remaining_after_fixed;
            let cramped = !unconstrained && hug_min_sum >= remaining_after_fixed;
            let slack = remaining_after_fixed - hug_min_sum;
            let total_range = hug_max_sum - hug_min_sum;

            for &HugBound { idx, lo, hi } in &self.hug_bounds {
                let v = if unconstrained {
                    hi
                } else if cramped {
                    lo
                } else if total_range > 0.0 {
                    (lo + slack * (hi - lo) / total_range).min(hi)
                } else {
                    lo
                };
                self.sizes[idx] = v;
                self.resolved.insert(idx);
                consumed += v;
            }
        }

        // Phase 3: Fill, over what Fixed and Hug left. Capping the
        // min-content floor at `Track.max` keeps the interval ordered when
        // a rigid descendant exceeds the explicit track cap.
        self.flexible.clear();
        for (i, t) in tracks.iter().enumerate() {
            if let Some(weight) = t.size.fill_weight() {
                self.flexible.push(FillItem::new(
                    i,
                    weight,
                    t.content_floor(hugs.min[i]),
                    t.max,
                ));
            }
        }
        FillItem::distribute(&mut self.flexible, (total - consumed).max(0.0));
        for item in &self.flexible {
            self.sizes[item.key] = item.size;
        }

        // Phase 4: commit Fill tracks as resolved when the grid's own axis
        // sizing guarantees measure-time `total` matches arrange-time slot.
        if commit_fill && total.is_finite() {
            for (i, t) in tracks.iter().enumerate() {
                if t.size.fill_weight().is_some() {
                    self.resolved.insert(i);
                }
            }
        }
    }
}
