//! Bounded set of screen-space damage rects produced by
//! [`super::Damage::compute`] and consumed by the encoder filter +
//! backend scissor.
//!
//! [`DamageRegion::add`] follows a four-step policy (in
//! `docs/roadmap/multi-rect-damage.md`):
//!
//! 1. Drop empty (zero-area) input.
//! 2. Drop input already contained by an existing rect.
//! 3. Cascade-absorb: while any existing rect is contained by the
//!    candidate OR fits the proximity-merge rule
//!    `bbox(A,B).area() ≤ MERGE_AREA_RATIO * (|A| + |B|)`, remove it
//!    from the array and grow the candidate by the union. The
//!    cascade is important — absorbing one rect may bring the
//!    candidate into overlap with another.
//! 4. Append if there's room; otherwise min-growth-merge into the
//!    existing rect whose union with the candidate adds the least
//!    area (Slint's `add_box` heuristic).
//!
//! Two unrelated tiny dirty corners stay as two distinct rects: the
//! bbox of the union is much larger than the sum of their areas, so
//! step 3 doesn't fire, and there's still room to append.

use crate::primitives::rect::Rect;
use tinyvec::ArrayVec;

/// Maximum disjoint damage rects retained per frame. The merge policy
/// (Step 2) guarantees `len ≤ DAMAGE_RECT_CAP`, so the inline storage
/// never spills.
pub(crate) const DAMAGE_RECT_CAP: usize = 8;

/// Proximity-merge ratio. Two rects collapse into one when the
/// bounding box's area is at most `MERGE_AREA_RATIO ×` the sum of
/// their individual areas — i.e. up to 30 % overdraw waste relative
/// to the actual changed area is acceptable. `1.0` reproduces the
/// strict LVGL rule (overlap or edge-touch only); `> 1.0` admits
/// near-but-not-overlapping pairs. Picked at 1.3 so axis-adjacent
/// damage (one cell + its immediate neighbour with a 2 px gap)
/// merges, but two cells more than one stride apart don't —
/// matches the GPU bench crossover (`damage_merge_gpu`) on Apple
/// Silicon. Tunable; see `docs/roadmap/damage-merge-research.md`.
pub(crate) const MERGE_AREA_RATIO: f32 = 1.3;

/// Set of damage rects, kept in screen space. `Copy` so
/// [`super::DamagePaint`] threads through `FrameOutput` and the
/// encoder by value without lifetimes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DamageRegion {
    rects: ArrayVec<[Rect; DAMAGE_RECT_CAP]>,
}

impl DamageRegion {
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
    /// diagonal-overlap path where the bbox-waste rule rejects the
    /// merge — rare and conservative (biases toward `Full` repaint
    /// at the boundary). Drives the full-repaint coverage check.
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
        let mut candidate = r;
        loop {
            let absorbed = self.rects.iter().position(|e| {
                candidate.contains_rect(*e)
                    || candidate.union(*e).area()
                        <= MERGE_AREA_RATIO * (candidate.area() + e.area())
            });
            match absorbed {
                Some(i) => {
                    let e = self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                }
                None => break,
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

/// Wrap a single rect — the region's `add` policy applies, so a
/// zero-area rect yields an empty region.
impl From<Rect> for DamageRegion {
    fn from(r: Rect) -> Self {
        let mut region = Self::default();
        region.add(r);
        region
    }
}

#[cfg(test)]
mod tests;
