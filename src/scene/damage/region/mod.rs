//! Bounded set of screen-space damage rects produced by
//! [`crate::scene::damage::DamageEngine::compute`] and consumed by the encoder filter +
//! backend scissor.
//!
//! Merge policy: agglomerative bottom-up clustering driven by the
//! Surface Area Heuristic (Walter et al., Cornell IRT 2008). For two
//! rects A and B the merge cost is
//! `cost = bbox(A,B).area() − A.area() − B.area()` — the extra
//! pixels that would be redrawn if the pair were collapsed (also
//! known as `union_excess`; identical to Iced's metric and the 2-D
//! restriction of SAH used for BVH builds). A pair merges when
//! `cost < budget_px` — the per-pass setup cost expressed in
//! "extra-overdraw pixels equivalent", passed to each fold rather than
//! stored on the result. The default ([`DEFAULT_PASS_BUDGET_PX`]) is
//! `DamageEngine`'s and the right knob for most callers.
//!
//! `add(r)` cluster-grows a candidate by repeatedly absorbing the
//! cheapest existing slot until no slot meets the budget, then
//! either appends or (at cap) forces the slot whose union with the
//! candidate adds the least area into the growing cluster (Slint's
//! `add_box`). The forced merge then resumes absorption so its grown
//! bbox cannot overlap another retained slot. Containment is just the
//! `cost ≤ −min(A,B).area()` limit of the same predicate, so it falls
//! out of the cluster-grow loop without a separate branch.
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
/// crossover on Apple Silicon sits near 7 000 px² for an isolated pair,
/// but real workloads
/// form clusters where each merge eliminates one *additional* pass —
/// the budget is per-pair-cost, so cluster total overdraw can run
/// somewhat higher in practice.
pub(crate) const DEFAULT_PASS_BUDGET_PX: f32 = 20_000.0;

/// Set of disjoint damage rects, kept in screen space. `Copy` so
/// [`crate::scene::damage::Damage`] threads through `FrameOutput` and the
/// encoder by value without lifetimes.
///
/// **The rects and nothing else.** The merge budget is an argument of
/// [`Self::add`], because it describes the fold rather than the result;
/// the damaged fraction of the surface rides
/// [`CollapsedDamage`](crate::scene::damage::region::CollapsedDamage),
/// because it means nothing without the surface it was measured against.
/// Neither has a reader past the merge — the encoder filter and the
/// backend scissors take rects — and holding them here is what forced this
/// type's equality to exclude a field it carried.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DamageRegion {
    /// Mutate only through [`Self::add`] — it owns the merge policy
    /// and the `len ≤ DAMAGE_RECT_CAP` invariant.
    pub(crate) rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]>,
}

/// One frame's damage after the merge: the bounded rect set, and how much
/// of the surface those rects cover.
///
/// The pair is produced together by [`DamageRegion::collapse_from`] and
/// consumed together — [`Damage::new`](crate::scene::damage::Damage::new)
/// classifies the frame off the coverage, and
/// `PresentStrategy::DirectAdaptive` reads the same number again to decide
/// whether a partial repaint is worth the backbuffer copy. Pairing them is
/// what keeps the ratio meaningful: it is measured against the surface the
/// rects were clipped to, and nothing downstream still has that surface to
/// re-derive it from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CollapsedDamage {
    pub(crate) region: DamageRegion,
    /// Damaged fraction of the surface (`total_area / surface_area`), in
    /// logical space on both sides so the ratio is DPI-independent.
    pub(crate) coverage: f32,
}

impl DamageRegion {
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
    pub(crate) fn collapse_from(rects: &[Rect], budget_px: f32, surface: Rect) -> CollapsedDamage {
        // A degenerate surface is a logic error — the host filters resize-to-zero
        // before damage runs. Asserting at the one site that divides by surface
        // area lets `Damage::new` stay a pure classifier (no surface needed).
        let surface_area = surface.area();
        debug_assert!(
            surface_area > EPS,
            "damage collapsed against a degenerate surface: {surface:?}"
        );
        let mut region = Self::default();
        for r in rects {
            // `add` re-gates on `is_paint_empty`, so the pre-check is
            // only an intersect-cost saver, not load-bearing.
            let clipped = r.clamp_to(surface);
            if !clipped.is_paint_empty() {
                region.add(clipped, budget_px);
            }
        }
        CollapsedDamage {
            coverage: region.total_area() / surface_area,
            region,
        }
    }

    pub(crate) fn iter_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.rects.iter().copied()
    }

    /// True if `r` intersects any rect in the region. Used by the
    /// encoder filter to gate per-leaf paint emission.
    pub(crate) fn any_intersects(&self, r: Rect) -> bool {
        self.rects.iter().any(|d| r.intersects(*d))
    }

    /// Sums per-rect areas. The merge policy collapses every overlapping
    /// pair before insertion completes, so no overlap subtraction is
    /// needed. Backs [`Self::collapse_from`]'s coverage seal. Region rects
    /// are surface-clipped at `collapse_from`, so this is already "visible
    /// area" — no extra intersect needed at the threshold site.
    fn total_area(&self) -> f32 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Fold `r` into the region per the policy described at the top
    /// of this module.
    ///
    /// `budget_px` is the per-pass setup cost the merge predicate spends,
    /// in "extra-overdraw pixels equivalent" — an argument rather than a
    /// field because it describes this fold, not the rects that come out
    /// of it. Pass `0.0` for strict-overlap-only merging.
    pub(crate) fn add(&mut self, r: Rect, budget_px: f32) {
        // `is_paint_empty`, not a bare `area() <= 0.0` — the shared
        // predicate also rejects NaN (which the bare compare admits,
        // poisoning every downstream intersects/cost comparison) and
        // sub-EPS slivers that paint nothing.
        if r.is_paint_empty() {
            return;
        }
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
                Some(i) if best_cost < budget_px => {
                    let e = self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                    continue;
                }
                _ => {}
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
            let e = self.rects.swap_remove(best_idx);
            candidate = candidate.union(e);
        }
    }
}

#[cfg(test)]
impl DamageRegion {
    /// These rects, with no coverage measured against any surface.
    ///
    /// For a case driving a consumer that reads only the rects — the
    /// encoder's damage filter, the backend's scissors — where the
    /// surface the ratio would be taken against does not exist.
    pub(crate) fn unmeasured(self) -> CollapsedDamage {
        CollapsedDamage {
            region: self,
            coverage: 0.0,
        }
    }
}

#[cfg(any(test, feature = "bench"))]
impl DamageRegion {
    /// Fold `rects` in order through [`Self::add`] with the default
    /// pass-budget.
    pub(crate) fn from_rects(rects: &[Rect]) -> Self {
        let mut region = Self::default();
        for r in rects {
            region.add(*r, DEFAULT_PASS_BUDGET_PX);
        }
        region
    }
}

/// Wrap a single rect with the default pass-budget. Gated with its
/// callers, which are tests only — narrower than
/// [`DamageRegion::from_rects`] above, which the damage bench also
/// drives. Production builds a region by folding the frame's damage
/// through [`DamageRegion::add`], never from one rect it already has.
#[cfg(test)]
impl From<Rect> for DamageRegion {
    fn from(r: Rect) -> Self {
        Self::from_rects(&[r])
    }
}

#[cfg(test)]
mod tests;
