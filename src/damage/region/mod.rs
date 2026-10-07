//! Bounded set of screen-space damage rects produced by [`crate::damage::engine::DamageEngine::compute`] and consumed by the encoder filter and backend scissors.
//!
//! Merge policy: agglomerative clustering by the Surface Area Heuristic. A pair merges when `bbox(A,B).area() − A.area() − B.area()` (the extra pixels redrawn) is below `budget_px`, the per-pass setup cost, passed to each fold; the default is [`DEFAULT_PASS_BUDGET_PX`].
//!
//! `add(r)` grows a candidate by absorbing the cheapest slot until none meets the budget, then appends or, at the cap, forces the cheapest merge and resumes absorbing so no retained slots overlap. Containment is the same predicate's limit.
//!
//! Intersecting pairs always merge regardless of budget: overlapping scissor passes would paint the overlap twice (`LoadOp::Load` each).
//!
//! Unrelated tiny corners stay distinct (huge excess); nearby clusters collapse gradually.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::EPS;
use tinyvec::ArrayVec;

/// Maximum disjoint damage rects retained per frame; the merge policy guarantees `len ≤ DAMAGE_RECT_CAP`, so inline storage never spills.
pub(crate) const DAMAGE_RECT_CAP: usize = 8;

/// Default per-pass setup cost, in extra-overdraw pixels. 20 000 px² collapses near pairs (axis-adjacent, animation-frame) without merging unrelated tiny corners; the isolated-pair GPU crossover is near 7 000 px², but clusters let each merge remove another pass.
pub(crate) const DEFAULT_PASS_BUDGET_PX: f32 = 20_000.0;

/// Set of disjoint damage rects in screen space. `Copy`, so [`crate::damage::Damage`] threads through `FrameOutput` and the encoder by value.
///
/// The rects and nothing else: the merge budget describes the fold, not the result, and the damaged fraction ([`CollapsedDamage`]) needs its surface.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DamageRegion {
    /// Private so [`Self::add`] is the only way in: it owns the merge policy and the `len ≤ DAMAGE_RECT_CAP` invariant.
    rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]>,
}

/// One frame's damage after the merge: the bounded rect set and the surface fraction it covers, produced and consumed together ([`Damage::new`](crate::damage::Damage::new), `PresentStrategy::DirectAdaptive`) since nothing downstream can re-derive the ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CollapsedDamage {
    pub(crate) region: DamageRegion,
    /// Damaged fraction of the surface (`total_area / surface_area`), in logical space on both sides so it is DPI-independent.
    pub(crate) coverage: f32,
}

impl DamageRegion {
    /// Build a region from `rects`, clipping each to `surface` first: off-surface pixels bias `FULL_REPAINT_THRESHOLD`, the encoder's `any_intersects` and the scissor, and source rects routinely overflow at high zoom.
    pub(crate) fn collapse_from(rects: &[Rect], budget_px: f32, surface: Rect) -> CollapsedDamage {
        let surface_area = surface.area();
        debug_assert!(
            surface_area > EPS,
            "damage collapsed against a degenerate surface: {surface:?}"
        );
        let mut region = Self::default();
        for r in rects {
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

    pub(crate) fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub(crate) fn any_intersects(&self, r: Rect) -> bool {
        self.rects.iter().any(|d| r.intersects(*d))
    }

    /// Sums per-rect areas; overlaps always merge and rects are surface-clipped, so no subtraction is needed.
    fn total_area(&self) -> f32 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Fold `r` into the region per the module policy. `budget_px` is an argument because it describes this fold, not the result.
    pub(crate) fn add(&mut self, r: Rect, budget_px: f32) {
        // `is_paint_empty`, not `area() <= 0.0`: it also rejects NaN (which would poison every comparison) and sub-EPS slivers.
        if r.is_paint_empty() {
            return;
        }
        let mut candidate = r;
        // Fused scan: early-out if a rect contains the candidate, note the first intersecting rect for an unconditional merge, and track the cheapest non-intersecting candidate for the budget grow. An intersection restarts the loop.
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
    /// These rects with no coverage measured, for consumers that read only rects.
    pub(crate) fn unmeasured(self) -> CollapsedDamage {
        CollapsedDamage {
            region: self,
            coverage: 0.0,
        }
    }
}

#[cfg(any(test, feature = "bench"))]
impl DamageRegion {
    pub(crate) fn from_rects(rects: &[Rect]) -> Self {
        let mut region = Self::default();
        for r in rects {
            region.add(*r, DEFAULT_PASS_BUDGET_PX);
        }
        region
    }
}

/// Wrap a single rect with the default budget. Gated with its callers (tests); production folds through [`DamageRegion::add`].
#[cfg(test)]
impl From<Rect> for DamageRegion {
    fn from(r: Rect) -> Self {
        Self::from_rects(&[r])
    }
}

#[cfg(test)]
mod tests;
