use crate::primitives::{approx, corners::Corners, size::Size, spacing::Spacing};
use core::f32::consts::FRAC_1_SQRT_2;
use glam::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Rect {
    pub min: Vec2,
    pub size: Size,
}

impl std::hash::Hash for Rect {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        approx::hash_rect(*self, state);
    }
}

impl Rect {
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        size: Size::ZERO,
    };

    #[inline]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            size: Size::new(w, h),
        }
    }

    #[inline]
    pub const fn from_min_max(min: Vec2, max: Vec2) -> Self {
        debug_assert!(min.x <= max.x && min.y <= max.y);
        Self {
            min,
            size: Size::new(max.x - min.x, max.y - min.y),
        }
    }

    #[inline]
    pub const fn max(&self) -> Vec2 {
        Vec2::new(self.min.x + self.size.w, self.min.y + self.size.h)
    }
    #[inline]
    pub const fn center(&self) -> Vec2 {
        Vec2::new(
            self.min.x + self.size.w * 0.5,
            self.min.y + self.size.h * 0.5,
        )
    }
    #[inline]
    pub const fn area(&self) -> f32 {
        self.size.w * self.size.h
    }

    /// True if this rect is approximately `Rect::ZERO` — `min` within
    /// `EPS` of `(0, 0)` AND `size.approx_zero()`. Strict, matches
    /// [`Size::approx_zero`] semantic.
    #[inline]
    pub const fn approx_zero(self) -> bool {
        use crate::primitives::approx::approx_zero;
        approx_zero(self.min.x) && approx_zero(self.min.y) && self.size.approx_zero()
    }

    /// True when this rect paints no pixels — at least one axis is
    /// `<= EPS` (including NaN / negative). Defers to
    /// [`Size::is_paint_empty`]; shared between every cmd-buffer
    /// noop gate so the predicate can't drift.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        self.size.is_paint_empty()
    }

    #[inline]
    pub const fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// True when `self` fully encloses `other`. Equality on the right
    /// edges counts (so `r.contains_rect(r)` is `true`). Used by the
    /// damage-region merge policy to drop rects already covered by a
    /// bigger one.
    #[inline]
    pub const fn contains_rect(&self, other: Self) -> bool {
        let self_max = self.max();
        let other_max = other.max();
        other.min.x >= self.min.x
            && other.min.y >= self.min.y
            && other_max.x <= self_max.x
            && other_max.y <= self_max.y
    }

    /// Outset by `amount` on each side, growing both edges. Symmetric
    /// counterpart to `deflated_by` for the common "uniform expansion"
    /// case (centred stroke painted-extent, AABB-around-circle, etc.).
    /// Negative input mirrors `deflated_by(Spacing::all(-amount))`
    /// without clamping — callers needing the size clamp should use
    /// `deflated_by` instead.
    #[inline]
    pub const fn inflated(&self, amount: f32) -> Self {
        Self {
            min: Vec2::new(self.min.x - amount, self.min.y - amount),
            size: Size::new(self.size.w + 2.0 * amount, self.size.h + 2.0 * amount),
        }
    }

    /// Largest axis-aligned rect that fits inside `self` when `self`
    /// is the bounding box of a rounded-rect paint with the given
    /// corner radii. Each side is inset by
    /// `max(adjacent_radii) * (1 - 1/√2)` — the 45° point of the
    /// corner arc, the deepest the inscribed rect can reach without
    /// crossing the rounded cutout. Returned size is clamped at
    /// zero; a sharp-cornered input passes through unchanged. Used
    /// by the renderer's occlusion-prune to derive the opaque cover
    /// area of a rounded fill.
    #[inline]
    pub fn inscribed_for_corners(&self, corners: Corners) -> Self {
        if corners.approx_zero() {
            return *self;
        }
        // `1 - 1/√2 ≈ 0.2929`: the inscribed-square offset per unit
        // radius for a quarter-circle arc. Multiplying a corner
        // radius by this gives the distance from the bounding-box
        // corner inward to the arc's 45° point.
        const KAPPA: f32 = 1.0 - FRAC_1_SQRT_2;
        // Single SIMD f16x4→f32x4 unpack — `tl()`/`tr()`/`br()`/`bl()`
        // would each issue an independent f16→f32 conversion.
        let [tl, tr, br, bl] = corners.as_array();
        let top = tl.max(tr) * KAPPA;
        let bottom = bl.max(br) * KAPPA;
        let left = tl.max(bl) * KAPPA;
        let right = tr.max(br) * KAPPA;
        Self {
            min: Vec2::new(self.min.x + left, self.min.y + top),
            size: Size::new(
                (self.size.w - left - right).max(0.0),
                (self.size.h - top - bottom).max(0.0),
            ),
        }
    }

    /// Inset by `s` on each side, clamping the resulting size at zero. Used for
    /// margin / padding insets in the layout pass.
    #[inline]
    pub fn deflated_by(&self, s: Spacing) -> Self {
        let [l, t, r, b] = s.as_array();
        Self {
            min: Vec2::new(self.min.x + l, self.min.y + t),
            size: Size::new(
                (self.size.w - (l + r)).max(0.0),
                (self.size.h - (t + b)).max(0.0),
            ),
        }
    }

    /// True if `self` and `other` overlap on both axes (strict — touching
    /// edges don't count). Used by the encoder's damage-rect filter to
    /// decide whether a node's paint commands can be skipped.
    #[inline]
    pub const fn intersects(&self, other: Self) -> bool {
        let a_max = self.max();
        let b_max = other.max();
        self.min.x < b_max.x
            && other.min.x < a_max.x
            && self.min.y < b_max.y
            && other.min.y < a_max.y
    }

    /// Axis-aligned intersection. Returns a zero-size rect if the inputs
    /// don't overlap (either dimension goes negative).
    #[inline]
    pub const fn intersect(&self, other: Self) -> Self {
        let (a, b) = (self.max(), other.max());
        let min = Vec2::new(self.min.x.max(other.min.x), self.min.y.max(other.min.y));
        let max = Vec2::new(a.x.min(b.x), a.y.min(b.y));
        Self {
            min,
            size: Size::new((max.x - min.x).max(0.0), (max.y - min.y).max(0.0)),
        }
    }

    /// Smallest axis-aligned rect enclosing both `self` and `other`. A
    /// paint-empty operand (any axis ≤ EPS, NaN included — see
    /// [`Self::is_paint_empty`]) acts as the identity, so callers can
    /// fold a `Rect::ZERO`-seeded accumulator without a special
    /// first-node branch and a non-painting extent can never drag a
    /// rollup's min to the origin. The integer-rectangle union follows the
    /// same contract. Fold over
    /// `Option<Rect>` only when "no rects at all" must stay
    /// distinguishable from "some rects".
    #[inline]
    pub const fn union(&self, other: Self) -> Self {
        if self.is_paint_empty() {
            return other;
        }
        if other.is_paint_empty() {
            return *self;
        }
        // `f32::min`/`max` rather than `Vec2::min`/`max` only because
        // glam's aren't `const fn`.
        let (a, b) = (self.max(), other.max());
        let min = Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y));
        let max = Vec2::new(a.x.max(b.x), a.y.max(b.y));
        Self {
            min,
            size: Size::new(max.x - min.x, max.y - min.y),
        }
    }

    /// Scale by `scale` and optionally snap edges to integer pixels. Used at
    /// the logical→physical-px boundary inside the renderer; snapping derives
    /// width/height from rounded edges (not from `size * scale`) to avoid
    /// creeping width drift across rows of identical rects.
    #[inline]
    pub const fn scaled_by(&self, scale: f32, snap: bool) -> Self {
        // Scalar lanes because glam's `Vec2` ops aren't `const fn`.
        let m = self.max();
        let mut min = Vec2::new(self.min.x * scale, self.min.y * scale);
        let mut max = Vec2::new(m.x * scale, m.y * scale);
        if snap {
            min = Vec2::new(min.x.round(), min.y.round());
            max = Vec2::new(max.x.round(), max.y.round());
        }
        Self {
            min,
            size: Size::new((max.x - min.x).max(0.0), (max.y - min.y).max(0.0)),
        }
    }
}

#[cfg(test)]
mod tests;
