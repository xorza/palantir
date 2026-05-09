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
//!    candidate OR fits the LVGL "merge if `bbox(A,B) ≤ |A|+|B|`"
//!    rule (i.e. overlapping or edge-touching), remove it from the
//!    array and grow the candidate by the union. The cascade is
//!    important — absorbing one rect may bring the candidate into
//!    overlap with another.
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
        // 1. Skip empty contributions.
        if r.area() <= 0.0 {
            return;
        }
        // 2. Skip if already covered.
        if self.rects.iter().any(|e| e.contains_rect(r)) {
            return;
        }
        // 3. Cascade-absorb: pull in any existing rect the candidate
        //    contains or merges with; each absorption grows the
        //    candidate, which may absorb more on the next pass.
        let mut candidate = r;
        loop {
            let absorbed = self.rects.iter().position(|e| {
                candidate.contains_rect(*e)
                    || candidate.union(*e).area() <= candidate.area() + e.area()
            });
            match absorbed {
                Some(i) => {
                    let e = self.rects.swap_remove(i);
                    candidate = candidate.union(e);
                }
                None => break,
            }
        }
        // 4. Append, or min-growth-merge if at cap.
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
