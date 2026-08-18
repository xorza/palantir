//! Settling one axis: how much room each track gets once the items across it
//! have been measured.

use crate::layout::axis::Axis;
use crate::layout::grid::measuring::resolve_fixed;
use crate::layout::grid::{AxisScratch, GridTrackStore, HugBound, HugRanges};
use crate::layout::support::weighted_share;
use crate::layout::types::layout_mode::GridDefId;
use crate::layout::types::track::Track;

/// Either copy persisted resolved sizes from the last measure or
/// re-run [`resolve_axis`] — whichever is sound for arrange's
/// `(grid, axis, slot)`. See the call-site comment for the
/// soundness conditions; the predicate here is just the boolean
/// version of those.
pub(super) fn resolve_or_reuse(
    a: &mut AxisScratch,
    tracks: &[Track],
    hugs: &mut GridTrackStore,
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
    if hugs.total_used(idx, axis) == Some(total) {
        a.sizes.copy_from_slice(hugs.sizes_slice(idx, axis));
        return;
    }
    resolve_axis(a, tracks, hugs.ranges(idx, axis), total, gap, false);
}

#[inline]
pub(super) fn content_floor(track: &Track, min_content: f32) -> f32 {
    min_content.max(track.min).min(track.max)
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
/// 3. **Fill:** original constraint-by-exclusion algorithm — Fill tracks
///    distribute leftover proportional to weight; any Fill whose share
///    falls outside its capped min-content floor and `Track.max` clamps
///    and exits the pool; remaining Fills rebalance.
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
    a: &mut AxisScratch,
    tracks: &[Track],
    hugs: HugRanges<'_>,
    total: f32,
    gap: f32,
    commit_fill: bool,
) {
    let n = tracks.len();
    a.sizes.fill(0.0);
    // Reset resolved flags. Fixed + Hug get marked resolved as they're
    // computed. Fill stays unresolved so cells in Fill cols see INF as
    // their available width via `known_span_size`, which is what makes
    // "Fill is finalized at arrange" hold. Without this, cells in
    // Fill cols would measure with measure-time Fill leftover (a
    // finite value), then arrange might assign a different
    // intrinsic-floor-driven slot to a Hug grid and the cell
    // rect/shape would disagree.
    a.resolved.clear();
    let total_gap = gap * n.saturating_sub(1) as f32;

    // Phase 1: Fixed.
    let mut consumed = total_gap + resolve_fixed(a, tracks);

    // Phase 2: Hug, constraint-solved against remaining-after-Fixed.
    // Single pass: snapshot each Hug track's clamped `(lo, hi)` once,
    // pick the distribution rule from the totals, then write sizes.
    a.hug_bounds.clear();
    let mut hug_min_sum = 0.0_f32;
    let mut hug_max_sum = 0.0_f32;
    for (i, t) in tracks.iter().enumerate() {
        if t.size.is_hug() {
            let lo = content_floor(t, hugs.min[i]);
            let hi = hugs.max[i].max(lo).min(t.max);
            hug_min_sum += lo;
            hug_max_sum += hi;
            a.hug_bounds.push(HugBound { idx: i, lo, hi });
        }
    }

    if !a.hug_bounds.is_empty() {
        let remaining_after_fixed = (total - consumed).max(0.0);
        // Pick distribution mode once. `unconstrained` covers infinite
        // total (Hug parent) and the "every Hug fits at max" case;
        // `cramped` covers "even at min the Hugs overflow"; otherwise
        // distribute slack proportional to per-track `(hi - lo)`.
        let unconstrained = total.is_infinite() || hug_max_sum <= remaining_after_fixed;
        let cramped = !unconstrained && hug_min_sum >= remaining_after_fixed;
        let slack = remaining_after_fixed - hug_min_sum;
        let total_range = hug_max_sum - hug_min_sum;

        for &HugBound { idx, lo, hi } in &a.hug_bounds {
            let v = if unconstrained {
                hi
            } else if cramped {
                lo
            } else if total_range > 0.0 {
                (lo + slack * (hi - lo) / total_range).min(hi)
            } else {
                lo
            };
            a.sizes[idx] = v;
            a.resolved.insert(idx);
            consumed += v;
        }
    }

    // Phase 3: Fill — constraint-by-exclusion. Fills get the leftover
    // after Fixed + Hug, distributed by weight; any Fill whose share
    // falls outside `[content_floor, Track.max]` clamps and exits the
    // pool, then remaining Fills rebalance. Capping the min-content floor
    // keeps the interval ordered when a rigid descendant exceeds the
    // explicit track cap. This mirrors the `[floor, cap]` freeze in
    // `stack::freeze_distribute` (kept in sync by hand; see its doc for
    // why the two aren't physically merged).
    let mut remaining = (total - consumed).max(0.0);
    a.flexible.clear();
    let mut flexible_weight = 0.0_f64;
    for (i, t) in tracks.iter().enumerate() {
        if let Some(weight) = t.size.fill_weight() {
            a.flexible.push(i);
            flexible_weight += f64::from(weight);
        }
    }

    // Clamp-and-rebalance loop. Each iteration looks for one Fill whose
    // proportional share violates `[lo, Track.max]`; if it exists,
    // clamp it, remove it from the pool, and rerun. When every
    // remaining Fill's share is in-range, commit them at that share and
    // exit. Converges in ≤ N iterations (each clamp removes one).
    while !a.flexible.is_empty() && flexible_weight > 0.0 {
        let clamp_idx = a.flexible.iter().position(|&i| {
            let t = &tracks[i];
            let weight = t.size.fill_weight().unwrap();
            let candidate = weighted_share(remaining, weight, flexible_weight);
            let lo = content_floor(t, hugs.min[i]);
            candidate < lo || candidate > t.max
        });
        match clamp_idx {
            Some(k) => {
                let i = a.flexible[k];
                let t = &tracks[i];
                let weight = t.size.fill_weight().unwrap();
                let candidate = weighted_share(remaining, weight, flexible_weight);
                let lo = content_floor(t, hugs.min[i]);
                let clamped = candidate.clamp(lo, t.max);
                a.sizes[i] = clamped;
                remaining = (remaining - clamped).max(0.0);
                flexible_weight -= f64::from(weight);
                a.flexible.swap_remove(k);
            }
            None => {
                for &i in a.flexible.iter() {
                    let weight = tracks[i].size.fill_weight().unwrap();
                    a.sizes[i] = weighted_share(remaining, weight, flexible_weight);
                }
                break;
            }
        }
    }

    // Phase 4: commit Fill tracks as resolved when the grid's own axis
    // sizing guarantees measure-time `total` matches arrange-time slot.
    if commit_fill && total.is_finite() {
        for (i, t) in tracks.iter().enumerate() {
            if t.size.fill_weight().is_some() {
                a.resolved.insert(i);
            }
        }
    }
}
