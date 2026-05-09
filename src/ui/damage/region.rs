//! Bounded set of screen-space damage rects produced by
//! [`super::Damage::compute`] and consumed by the encoder filter +
//! backend scissor.
//!
//! Step 1 of the multi-rect-damage roadmap (`docs/roadmap/multi-rect-
//! damage.md`): the storage is sized for the eventual N=8 cap, but
//! [`DamageRegion::add`] currently keeps `len ≤ 1` by unioning every
//! contribution into slot 0 — behaviour is bit-identical to the
//! pre-existing `Option<Rect>` accumulator. Step 2 swaps in the LVGL
//! merge rule + min-growth fallback that lets the structure actually
//! hold multiple disjoint rects.

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

    pub(crate) fn iter(&self) -> impl Iterator<Item = Rect> + '_ {
        self.rects.iter().copied()
    }

    /// True if `r` intersects any rect in the region. Used by the
    /// encoder filter to gate per-leaf paint emission.
    pub(crate) fn any_intersects(&self, r: Rect) -> bool {
        self.rects.iter().any(|d| r.intersects(*d))
    }

    /// Sum of per-rect areas. Step 1 keeps `len ≤ 1`, so this is the
    /// single rect's area; Step 2 makes it the (possibly over-counted)
    /// total used for the full-repaint coverage check.
    pub(crate) fn total_area(&self) -> f32 {
        self.rects.iter().map(|r| r.area()).sum()
    }

    /// Fold `r` into the region. Step 1 policy: if empty, push;
    /// otherwise union with slot 0. Bit-identical to the previous
    /// `extend(&mut Option<Rect>, r)` helper.
    pub(crate) fn add(&mut self, r: Rect) {
        if self.rects.is_empty() {
            self.rects.push(r);
        } else {
            self.rects[0] = self.rects[0].union(r);
        }
    }
}
