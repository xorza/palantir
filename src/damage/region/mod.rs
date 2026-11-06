//! Bounded set of screen-space damage rects produced by [`crate::damage::engine::DamageEngine::compute`] and consumed by the encoder filter and backend scissors.
//!
//! Merge policy: agglomerative clustering by the Surface Area Heuristic. A pair merges when `bbox(A,B).area() − A.area() − B.area() + shared` (the extra pixels redrawn) is below [`MergePolicy::budget_px`], the per-pass setup cost, passed to each fold; the shipped values are [`MergePolicy::DEFAULT`].
//!
//! `add(r)` grows a candidate by absorbing the cheapest slot until none meets the budget, then appends or, at [`MergePolicy::rect_cap`], forces the cheapest merge and resumes absorbing. A rect inside another goes at any budget.
//!
//! Overlapping pairs are no special case: merged when their bounds waste little, and kept apart otherwise, as a crosshair's strips are, whose bounds are the whole surface. The repaint's scissors take the pixels they share once ([`RepaintScissors`](crate::renderer::repaint_scissors::RepaintScissors)). Refused: always merging them, which repainted a 4 % crosshair whole, 19 to 105 % past the cheapest frame on four GPUs (`cargo damage-sweep`).
//!
//! Unrelated tiny corners stay distinct (huge excess); nearby clusters collapse gradually.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain::EPS;
use tinyvec::ArrayVec;

/// Maximum damage rects retained per frame; the merge policy guarantees `len ≤ DAMAGE_RECT_CAP`, so inline storage never spills.
pub(crate) const DAMAGE_RECT_CAP: usize = 8;

/// Default per-pass setup cost, in extra-overdraw pixels. 5 000 px² collapses near pairs (axis-adjacent, animation-frame) without merging unrelated tiny corners; the mean regret lies flat from 1 000 to 5 000 and rises past it on four GPUs (`cargo damage-sweep`).
const DEFAULT_PASS_BUDGET_PX: f32 = 5_000.0;

/// What a fold trades: the extra pixels one merge may redraw, and how many rects it keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MergePolicy {
    /// Per-pass setup cost, in extra-overdraw pixels: a pair whose merge redraws fewer merges.
    pub(crate) budget_px: f32,
    /// The most rects the fold keeps, from 1 to [`DAMAGE_RECT_CAP`].
    pub(crate) rect_cap: usize,
}

impl MergePolicy {
    /// What every frame folds by.
    pub(crate) const DEFAULT: Self = Self {
        budget_px: DEFAULT_PASS_BUDGET_PX,
        rect_cap: DAMAGE_RECT_CAP,
    };
}

/// Set of damage rects in screen space, which may overlap. `Copy`, so [`crate::damage::Damage`] threads through `FrameOutput` and the encoder by value.
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
    /// Damaged fraction of the surface (`total_area / surface_area`), each pixel once, in logical space on both sides so it is DPI-independent.
    pub(crate) coverage: f32,
}

impl DamageRegion {
    /// Build a region from `rects`, clipping each to `surface` first: off-surface pixels bias `FULL_REPAINT_THRESHOLD`, the encoder's `any_intersects` and the scissor, and source rects routinely overflow at high zoom.
    pub(crate) fn collapse_from(
        rects: &[Rect],
        policy: MergePolicy,
        surface: Rect,
    ) -> CollapsedDamage {
        let surface_area = surface.area();
        debug_assert!(
            surface_area > EPS,
            "damage collapsed against a degenerate surface: {surface:?}"
        );
        let mut region = Self::default();
        for r in rects {
            let clipped = r.clamp_to(surface);
            if !clipped.is_paint_empty() {
                region.add(clipped, policy);
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

    /// The area the rects cover, each pixel once, as they are surface-clipped: their sum, less what
    /// they share, in strips between the x edges where any two overlap. Bounded by the cap, and
    /// with no overlap, as most frames have, one pass of the pairs.
    fn total_area(&self) -> f32 {
        let rects = self.rects.as_slice();
        let overlapped = rects
            .iter()
            .enumerate()
            .any(|(i, a)| rects[i + 1..].iter().any(|b| a.intersects(*b)));
        if !overlapped {
            return rects.iter().map(|r| r.area()).sum();
        }
        let mut edges: ArrayVec<[f32; 2 * DAMAGE_RECT_CAP]> = ArrayVec::new();
        for r in rects {
            edges.push(r.min.x);
            edges.push(r.max().x);
        }
        edges.sort_unstable_by(f32::total_cmp);
        let mut area = 0.0;
        for strip in edges.windows(2) {
            let mut spans: ArrayVec<[(f32, f32); DAMAGE_RECT_CAP]> = rects
                .iter()
                .filter(|r| r.min.x <= strip[0] && r.max().x >= strip[1])
                .map(|r| (r.min.y, r.max().y))
                .collect();
            spans.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            let mut covered = 0.0;
            let mut end = f32::NEG_INFINITY;
            for (from, to) in spans {
                let from = from.max(end);
                if to > from {
                    covered += to - from;
                    end = to;
                }
            }
            area += covered * (strip[1] - strip[0]);
        }
        area
    }

    /// Fold `r` into the region per the module policy, which is an argument because it describes this fold, not the result.
    pub(crate) fn add(&mut self, r: Rect, policy: MergePolicy) {
        debug_assert!(
            (1..=DAMAGE_RECT_CAP).contains(&policy.rect_cap),
            "a rect cap of {} outside the storage",
            policy.rect_cap
        );
        // `is_paint_empty`, not `area() <= 0.0`: it also rejects NaN (which would poison every comparison) and sub-EPS slivers.
        if r.is_paint_empty() {
            return;
        }
        let mut candidate = r;
        // Fused scan: early-out if a rect contains the candidate, note the first rect it contains for an unconditional merge, and track the cheapest rect for the budget grow. A merge restarts the loop.
        loop {
            let mut swallowed_idx: Option<usize> = None;
            let mut best_idx: Option<usize> = None;
            let mut best_cost = f32::INFINITY;
            let cand_area = candidate.area();
            for (i, e) in self.rects.iter().enumerate() {
                let e = *e;
                if e.contains_rect(candidate) {
                    return;
                }
                if candidate.contains_rect(e) {
                    swallowed_idx = Some(i);
                    break;
                }
                let shared = if candidate.intersects(e) {
                    candidate.clamp_to(e).area()
                } else {
                    0.0
                };
                let cost = candidate.union(e).area() - cand_area - e.area() + shared;
                if cost < best_cost {
                    best_cost = cost;
                    best_idx = Some(i);
                }
            }
            if let Some(i) = swallowed_idx {
                let e = self.rects.swap_remove(i);
                candidate = candidate.union(e);
                continue;
            }
            match best_idx {
                Some(i) if best_cost < policy.budget_px => {
                    let e = self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                    continue;
                }
                _ => {}
            }
            if self.rects.len() < policy.rect_cap {
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::damage::region::{CollapsedDamage, DamageRegion, MergePolicy};
    use crate::primitives::geometry::rect::Rect;
    use tinyvec::ArrayVec;

    impl DamageRegion {
        pub(crate) fn from_rects(rects: &[Rect]) -> Self {
            let mut region = Self::default();
            for r in rects {
                region.add(*r, MergePolicy::DEFAULT);
            }
            region
        }

        /// A region of exactly `rects`, with no fold, so a damage sweep can measure a split no fold
        /// keeps.
        pub(crate) fn from_unmerged(rects: &[Rect]) -> Self {
            Self {
                rects: rects.iter().copied().collect::<ArrayVec<_>>(),
            }
        }

        /// These rects with no coverage measured, for consumers that read only rects.
        pub(crate) const fn unmeasured(self) -> CollapsedDamage {
            CollapsedDamage {
                region: self,
                coverage: 0.0,
            }
        }
    }

    impl MergePolicy {
        /// The default with a budget of `budget_px`.
        #[cfg(test)]
        pub(crate) const fn with_budget(budget_px: f32) -> Self {
            Self {
                budget_px,
                ..Self::DEFAULT
            }
        }
    }

    /// Wrap a single rect with the default budget; production folds through [`DamageRegion::add`].
    #[cfg(test)]
    impl From<Rect> for DamageRegion {
        fn from(r: Rect) -> Self {
            Self::from_rects(&[r])
        }
    }
}

#[cfg(test)]
mod tests;
