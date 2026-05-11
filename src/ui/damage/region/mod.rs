//! Bounded set of screen-space damage rects produced by
//! [`super::DamageEngine::compute`] and consumed by the encoder filter +
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
/// [`super::Damage`] threads through `FrameOutput` and the
/// encoder by value without lifetimes. The `budget_px` field drives
/// the merge predicate — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DamageRegion {
    rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]>,
    pub(crate) budget_px: f32,
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
        }
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
    /// reach this sum, so the only way to over-count is the
    /// diagonal-overlap path where the budget rejects the merge —
    /// rare and conservative (biases toward `Full` repaint at the
    /// boundary). Drives the full-repaint coverage check.
    pub(crate) fn total_area(&self) -> f32 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Fold `r` into the region per the policy described at the top
    /// of this module.
    pub(crate) fn add(&mut self, r: Rect) {
        if r.area() <= 0.0 {
            return;
        }
        if self.rects.iter().any(|e| e.contains_rect(r)) {
            return;
        }
        let budget = self.budget_px;
        let mut candidate = r;
        loop {
            if let Some(i) = self.rects.iter().position(|e| candidate.intersects(*e)) {
                let e = self.rects.swap_remove(i);
                candidate = candidate.union(e);
                continue;
            }
            let best = self
                .rects
                .iter()
                .enumerate()
                .map(|(i, e)| (i, *e, merge_cost(candidate, *e)))
                .min_by(|a, b| a.2.total_cmp(&b.2));
            match best {
                Some((i, e, cost)) if cost < budget => {
                    self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                }
                _ => break,
            }
        }
        if self.rects.len() < DAMAGE_RECT_CAP {
            self.rects.push(candidate);
            return;
        }
        let i = self
            .rects
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let growth_a = a.union(candidate).area() - a.area();
                let growth_b = b.union(candidate).area() - b.area();
                growth_a.total_cmp(&growth_b)
            })
            .map(|(i, _)| i)
            .expect("DAMAGE_RECT_CAP > 0");
        self.rects[i] = self.rects[i].union(candidate);
    }
}

/// SAH-style merge cost: extra pixels overdrawn if A and B were
/// collapsed into their bbox. Negative when one rect contains the
/// other (union = larger rect → cost = −smaller.area()), zero on
/// edge-touch, positive on disjoint pairs.
#[inline]
fn merge_cost(a: Rect, b: Rect) -> f32 {
    a.union(b).area() - a.area() - b.area()
}

/// Wrap a single rect with the default pass-budget.
impl From<Rect> for DamageRegion {
    fn from(r: Rect) -> Self {
        let mut region = Self::default();
        region.add(r);
        region
    }
}

#[cfg(test)]
mod tests;
