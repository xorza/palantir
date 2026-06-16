//! Bounded set of screen-space damage rects produced by
//! [`crate::ui::damage::DamageEngine::compute`] and consumed by the encoder filter +
//! backend scissor.
//!
//! Merge policy: agglomerative bottom-up clustering driven by the
//! Surface Area Heuristic (Walter et al., Cornell IRT 2008). For two
//! rects A and B the merge cost is
//! `cost = bbox(A,B).area() − A.area() − B.area()` — the extra
//! pixels that would be redrawn if the pair were collapsed (also
//! known as `union_excess`; identical to Iced's metric and the 2-D
//! restriction of SAH used for BVH builds). A pair merges when
//! `cost < self.budget_px` — the per-pass setup cost expressed in
//! "extra-overdraw pixels equivalent". Each `DamageRegion` carries
//! its own budget; the default ([`DEFAULT_PASS_BUDGET_PX`]) ships
//! with `DamageEngine`'s region and is the right knob for most callers.
//!
//! `add(r)` cluster-grows a candidate by repeatedly absorbing the
//! cheapest existing slot until no slot meets the budget, then
//! either appends or (at cap) min-growth-merges into the slot whose
//! union with the candidate adds the least area (Slint's
//! `add_box`). Containment is just the `cost ≤ −min(A,B).area()`
//! limit of the same predicate, so it falls out of the cluster-grow
//! loop without a separate branch.
//!
//! Intersecting pairs are always merged, regardless of budget —
//! two overlapping scissor passes would paint the overlap region
//! twice (`LoadOp::Load` on each), so merging is strictly cheaper
//! per-overlap-pixel even when the bbox grows. This is the LVGL
//! strict-overlap rule layered under the SAH proximity merge.
//!
//! Two unrelated tiny dirty corners stay distinct: their
//! union_excess is enormous (≈ surface_area) so the loop rejects
//! them. A cluster of N nearby rects collapses gradually as each
//! absorption grows the candidate's area, reducing the next
//! candidate-vs-existing cost.
//!
//! See `docs/roadmap/damage-merge-research.md` for cost-model
//! derivation and `multi-rect-damage.md` for the wider design
//! survey.

use crate::primitives::approx::EPS;
use crate::primitives::rect::Rect;
use tinyvec::ArrayVec;

/// Maximum disjoint damage rects retained per frame. The merge
/// policy guarantees `len ≤ DAMAGE_RECT_CAP`, so the inline storage
/// never spills.
pub(crate) const DAMAGE_RECT_CAP: usize = 8;

/// Default per-pass setup cost in "extra overdraw pixels
/// equivalent". A pair (A, B) merges when
/// `bbox(A,B).area() − A.area() − B.area() < budget`. Tuned at
/// 20 000 px² — same value as Iced; high enough to collapse near
/// pairs (axis-adjacent, gap-of-one-stride, animation-frame pairs)
/// without merging two unrelated tiny corners. The 2-cell GPU-bench
/// crossover on Apple Silicon (`docs/roadmap/damage-merge-research.md`)
/// sits near 7 000 px² for an isolated pair, but real workloads
/// form clusters where each merge eliminates one *additional* pass —
/// the budget is per-pair-cost, so cluster total overdraw can run
/// somewhat higher in practice.
pub(crate) const DEFAULT_PASS_BUDGET_PX: f32 = 20_000.0;

/// Set of damage rects, kept in screen space. `Copy` so
/// [`crate::ui::damage::Damage`] threads through `FrameOutput` and the
/// encoder by value without lifetimes. The `budget_px` field drives
/// the merge predicate — see the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DamageRegion {
    rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]>,
    pub(crate) budget_px: f32,
    /// Damaged fraction of the surface (`total_area / surface_area`),
    /// precomputed by [`Self::collapse_from`] against the surface its rects were
    /// clipped to — so the coverage thresholds that read it
    /// (`FULL_REPAINT_THRESHOLD` in the damage engine, `DIRECT_PROMOTE_COVERAGE`
    /// in the renderer) get a ready value instead of every caller threading the
    /// surface area back in. `0.0` on a region built any other way (`default` /
    /// `with_budget` / `From<Rect>`); those never reach a coverage check.
    /// Excluded from [`PartialEq`] (a derived cache, not identity — two regions
    /// covering the same rects are equal regardless of coverage).
    pub(crate) coverage: f32,
}

impl PartialEq for DamageRegion {
    /// Geometric identity only: same rects, same merge budget. The cached
    /// [`Self::coverage`] is a sealed-surface derivative, not part of what the
    /// region *is*, so an unsealed expected value still matches a sealed actual.
    fn eq(&self, other: &Self) -> bool {
        self.rects == other.rects && self.budget_px == other.budget_px
    }
}

impl Default for DamageRegion {
    fn default() -> Self {
        Self::with_budget(DEFAULT_PASS_BUDGET_PX)
    }
}

impl DamageRegion {
    /// Empty region with the merge predicate's pass-budget set
    /// explicitly (in extra-overdraw pixels). Pass `0.0` for
    /// strict-overlap-only merging.
    pub(crate) fn with_budget(budget_px: f32) -> Self {
        Self {
            rects: ArrayVec::new(),
            budget_px,
            coverage: 0.0,
        }
    }

    /// Build a region from `rects`, clipping each to `surface` before
    /// folding it through `add`. Off-surface pixels can never be
    /// painted, so storing them in the region biases every downstream
    /// consumer wrong: the `FULL_REPAINT_THRESHOLD` check in
    /// `Damage::new` would count them against the budget, the
    /// encoder's `any_intersects` filter would compare against a
    /// rect bigger than the viewport, and the GPU scissor would be
    /// asked to paint pixels off-screen. Source rects (paint_rects on
    /// root-level transformed canvases with no clip ancestor — see
    /// `cascade::compute_paint_rect`) routinely overflow at high zoom,
    /// so the clip is mandatory at the chokepoint, not optional at
    /// individual callsites.
    pub(crate) fn collapse_from(rects: &[Rect], budget_px: f32, surface: Rect) -> Self {
        // A degenerate surface is a logic error — the host filters resize-to-zero
        // before damage runs. Asserting at the one site that divides by surface
        // area lets `Damage::new` stay a pure classifier (no surface needed).
        let surface_area = surface.area();
        assert!(
            surface_area > EPS,
            "damage collapsed against a degenerate surface: {surface:?}"
        );
        let mut region = Self::with_budget(budget_px);
        for r in rects {
            let clipped = r.intersect(surface);
            if clipped.area() > 0.0 {
                region.add(clipped);
            }
        }
        // Seal coverage against the surface its rects were clipped to — both in
        // logical space, so the ratio is DPI-independent.
        region.coverage = region.total_area() / surface_area;
        region
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub(crate) fn iter_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.rects.iter().copied()
    }

    /// True if `r` intersects any rect in the region. Used by the
    /// encoder filter to gate per-leaf paint emission.
    pub(crate) fn any_intersects(&self, r: Rect) -> bool {
        self.rects.iter().any(|d| r.intersects(*d))
    }

    /// Sums per-rect areas without subtracting overlap. The merge
    /// policy collapses overlapping pairs into one rect before they
    /// reach this sum, so over-count arises only from two rare paths:
    /// the diagonal-overlap case where the budget rejects the merge,
    /// and the at-cap force-merge in `add` (which grows the min-growth
    /// slot and can leave it overlapping a neighbour). Both are
    /// conservative — they bias toward a `Full` repaint at the
    /// boundary. Backs [`Self::collapse_from`]'s coverage seal. Region rects are
    /// surface-clipped at `collapse_from`, so this is already "visible
    /// area" — no extra intersect needed at the threshold site.
    fn total_area(&self) -> f32 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Fold `r` into the region per the policy described at the top
    /// of this module.
    pub(crate) fn add(&mut self, r: Rect) {
        if r.area() <= 0.0 {
            return;
        }
        let budget = self.budget_px;
        let mut candidate = r;
        // Fused scan: in one pass over `self.rects` we (a) early-out if
        // an existing rect already contains the candidate, (b) note
        // the first intersecting rect for unconditional merge, and
        // (c) track the cheapest non-intersecting merge candidate for
        // the budget-driven cluster grow. Intersection short-circuits
        // — we restart the loop with the grown candidate.
        loop {
            let mut intersect_idx: Option<usize> = None;
            let mut best_idx: Option<usize> = None;
            let mut best_cost = f32::INFINITY;
            let cand_area = candidate.area();
            for (i, e) in self.rects.iter().enumerate() {
                let e = *e;
                if e.contains_rect(candidate) {
                    return;
                }
                if candidate.intersects(e) {
                    intersect_idx = Some(i);
                    break;
                }
                let cost = candidate.union(e).area() - cand_area - e.area();
                if cost < best_cost {
                    best_cost = cost;
                    best_idx = Some(i);
                }
            }
            if let Some(i) = intersect_idx {
                let e = self.rects.swap_remove(i);
                candidate = candidate.union(e);
                continue;
            }
            match best_idx {
                Some(i) if best_cost < budget => {
                    let e = self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                }
                _ => break,
            }
        }
        if self.rects.len() < DAMAGE_RECT_CAP {
            self.rects.push(candidate);
            return;
        }
        let mut best_idx = 0usize;
        let mut best_growth = f32::INFINITY;
        for (i, e) in self.rects.iter().enumerate() {
            let growth = e.union(candidate).area() - e.area();
            if growth < best_growth {
                best_growth = growth;
                best_idx = i;
            }
        }
        self.rects[best_idx] = self.rects[best_idx].union(candidate);
    }
}

/// Wrap a single rect with the default pass-budget.
impl From<Rect> for DamageRegion {
    fn from(r: Rect) -> Self {
        let mut region = Self::default();
        region.add(r);
        region
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::primitives::rect::Rect;
    use crate::ui::damage::region::*;

    /// `DamageRegion` rect count after adding `rects` in order (merge policy runs).
    /// Free fn rather than `impl DamageRegion` because `DamageRegion` is
    /// `pub(crate)` — external benches can't name the type to invoke an
    /// associated fn on it. Keep here so benches see a single namespace.
    pub fn region_after_adds(rects: &[Rect]) -> usize {
        let mut region = DamageRegion::default();
        for r in rects {
            region.add(*r);
        }
        region.iter_rects().count()
    }
}

#[cfg(test)]
mod tests;
