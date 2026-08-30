//! The logical-pixel rectangle every pass measures, arranges, clips and
//! damages in, plus the NaN-safe fold that derives one from a set of
//! points.

pub(crate) mod aabb;

use crate::primitives::approx::canon_bits;
use crate::primitives::nan::{self, NanCheck};
use crate::primitives::{
    approx::FloatHash, corners::Corners, num::F32Ext, size::Size, spacing::Spacing,
};
use core::f32::consts::FRAC_1_SQRT_2;
use glam::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
/// An axis-aligned rectangle in logical pixels, stored as origin + extent
/// rather than two corners — layout produces sizes, so this is the form
/// that avoids a subtraction on every read.
///
/// Half-open on both axes: [`Self::contains`] accepts the min edge and
/// rejects the max, so adjacent rects tile without double-hitting a
/// pointer on the seam. Hashing is approximate (`1e-4` tolerance).
pub struct Rect {
    /// Top-left corner.
    pub min: Vec2,
    /// Extent from [`Self::min`]. The bottom-right corner is
    /// [`Self::max`].
    pub size: Size,
}

impl std::hash::Hash for Rect {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// Origin then extent, each packed by its own impl — so a rect and the
/// `(Vec2, Size)` pair it is made of feed a hasher the same bytes.
impl FloatHash for Rect {
    #[inline]
    fn hash_eq<H: std::hash::Hasher>(&self, state: &mut H) {
        self.min.hash_eq(state);
        self.size.hash_eq(state);
    }

    #[inline]
    fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        self.min.hash_visual(state);
        self.size.hash_visual(state);
    }
}

impl Rect {
    /// Origin at `(0, 0)` with zero extent.
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        size: Size::ZERO,
    };

    /// The poisoned rect an [`Aabb`](aabb::Aabb) folds to once it has
    /// seen a NaN. [`Self::is_paint_empty`] reports it as invisible, so
    /// it drops out at the first no-op gate it meets.
    pub(crate) const NAN: Self = Self {
        min: Vec2::NAN,
        size: Size::new(f32::NAN, f32::NAN),
    };

    /// A rect from its top-left corner and extent.
    #[inline]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            size: Size::new(w, h),
        }
    }

    /// A rect from two corners.
    ///
    /// # Panics
    ///
    /// Debug-asserts that `min` is componentwise `<= max`, unless a
    /// corner is NaN — see the note in the body.
    #[inline]
    pub const fn from_min_max(min: Vec2, max: Vec2) -> Self {
        // A NaN corner is exempt, because it is *expected* input under
        // the AABB NaN contract (see [`Aabb`](aabb::Aabb)): a NaN vertex
        // is deliberately carried into the bounds so the shape-level
        // gate can drop the draw and name the shape it came from.
        // Tripping here instead would report the arithmetic and not the
        // caller. `max - min` keeps the NaN in `size`, which is what
        // `is_paint_empty` reads.
        debug_assert!(
            (min.x <= max.x && min.y <= max.y) || nan::vec2_has_nan(min) || nan::vec2_has_nan(max)
        );
        Self {
            min,
            size: Size::new(max.x - min.x, max.y - min.y),
        }
    }

    /// The four visually-canonical lanes [`FloatHash::hash_visual`]
    /// writes, as data instead.
    ///
    /// For a caller that packs them into a POD blob and hashes that in
    /// one write — the cascade's per-frame prefix — so it canonicalizes a
    /// rect the way everything else does without writing `canon_bits` out
    /// four times.
    #[inline]
    pub(crate) fn canon_lanes(self) -> [u32; 4] {
        [
            canon_bits(self.min.x),
            canon_bits(self.min.y),
            canon_bits(self.size.w),
            canon_bits(self.size.h),
        ]
    }

    /// Bottom-right corner — exclusive, per the half-open convention.
    #[inline]
    pub const fn max(self) -> Vec2 {
        Vec2::new(self.min.x + self.size.w, self.min.y + self.size.h)
    }
    /// Midpoint of the rect.
    #[inline]
    pub const fn center(self) -> Vec2 {
        Vec2::new(
            self.min.x + self.size.w * 0.5,
            self.min.y + self.size.h * 0.5,
        )
    }
    /// `width * height`.
    #[inline]
    pub const fn area(self) -> f32 {
        self.size.w * self.size.h
    }

    /// True when this rect paints no pixels — at least one axis is
    /// `<= EPS` (including NaN / negative). Defers to
    /// [`Size::is_paint_empty`]; shared between every paint-payload
    /// noop gate so the predicate can't drift.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        // `min` needs the NaN half of the test but not the `<= EPS`
        // half — a rect at a negative origin paints fine, one at an
        // undefined origin does not.
        self.size.is_paint_empty() || nan::vec2_has_nan(self.min)
    }

    /// True if any of the four lanes is NaN. `const`, so the const
    /// predicates that need the sweep can call it; the [`NanCheck`] impl
    /// below delegates here rather than keeping a second copy of the
    /// field walk.
    ///
    /// [`NanCheck`]: crate::primitives::nan::NanCheck
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        nan::vec2_has_nan(self.min) || self.size.has_nan()
    }

    /// Half-open containment: the min edges are inside, the max edges are
    /// not, so tiled rects never both claim the same point.
    #[inline]
    pub const fn contains(self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// True when `self` fully encloses `other`. Equality on the right
    /// edges counts (so `r.contains_rect(r)` is `true`). Used by the
    /// damage-region merge policy to drop rects already covered by a
    /// bigger one.
    #[inline]
    pub const fn contains_rect(self, other: Self) -> bool {
        let self_max = self.max();
        let other_max = other.max();
        other.min.x >= self.min.x
            && other.min.y >= self.min.y
            && other_max.x <= self_max.x
            && other_max.y <= self_max.y
    }

    /// Outset by `amount` on each side, growing both edges — the
    /// "uniform expansion" case (centred stroke painted-extent,
    /// AABB-around-circle). Counterpart to [`Self::deflated`], which is
    /// the same step inward and clamps where this one does not.
    #[inline]
    pub const fn inflated(self, amount: f32) -> Self {
        Self {
            min: Vec2::new(self.min.x - amount, self.min.y - amount),
            size: Size::new(self.size.w + 2.0 * amount, self.size.h + 2.0 * amount),
        }
    }

    /// Inset by `amount` on each side, clamping the resulting size at
    /// zero. The counterpart to [`Self::inflated`], and the uniform case
    /// of [`Self::deflated_by`].
    ///
    /// Clamped where [`Self::inflated`] is not, because the two ends are
    /// not symmetric: growing a rect cannot collapse it, and an inset
    /// deeper than the extent has no rect to name.
    #[inline]
    pub fn deflated(self, amount: f32) -> Self {
        Self {
            min: Vec2::new(self.min.x + amount, self.min.y + amount),
            size: Size::new(
                (self.size.w - 2.0 * amount).max(0.0),
                (self.size.h - 2.0 * amount).max(0.0),
            ),
        }
    }

    /// The axis-aligned square of half-extent `half` about `center`.
    #[inline]
    pub(crate) fn square_about(center: Vec2, half: f32) -> Self {
        Self {
            min: Vec2::new(center.x - half, center.y - half),
            size: Size::new(2.0 * half, 2.0 * half),
        }
    }

    /// Owner-local point a shape inside this rect spins about: the rect's
    /// centre in the owner-local space the shape's geometry is recorded
    /// in, so the rect's own `min` is no part of it.
    ///
    /// Both ends of the spin contract derive the pivot here — the encoder
    /// to put it in the payload the composer turns points by, the cascade
    /// to cover the disc the shape sweeps.
    #[inline]
    pub(crate) fn spin_pivot(self) -> Vec2 {
        Vec2::new(self.size.w * 0.5, self.size.h * 0.5)
    }

    /// Distance from `pivot` to this rect's farthest corner — the radius
    /// of the disc it sweeps when it turns about that point.
    #[inline]
    pub(crate) fn spun_radius(self, pivot: Vec2) -> f32 {
        (self.min - pivot)
            .abs()
            .max((self.max() - pivot).abs())
            .length()
    }

    /// The axis-aligned square this rect covers at *every* rotation about
    /// `pivot`.
    ///
    /// Angle-free by construction, which is the point: the composer culls
    /// a spun shape against this square and the cascade damages the same
    /// one, without the two passes having to sample the animation at the
    /// same instant.
    #[inline]
    pub(crate) fn spun_cover(self, pivot: Vec2) -> Self {
        Self::square_about(pivot, self.spun_radius(pivot))
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
    pub fn inscribed_for_corners(self, corners: Corners) -> Self {
        if corners.approx_zero() {
            return self;
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

    /// Outset by `s` on each side, growing both edges. The per-side
    /// counterpart to [`Self::inflated`], and what undoes a
    /// [`Self::deflated_by`] inset side for side, as long as that inset
    /// did not clamp.
    #[inline]
    pub fn inflated_by(self, s: Spacing) -> Self {
        let [l, t, r, b] = s.as_array();
        Self {
            min: Vec2::new(self.min.x - l, self.min.y - t),
            size: Size::new(self.size.w + (l + r), self.size.h + (t + b)),
        }
    }

    /// Inset by `s` on each side, clamping the resulting size at zero. Used for
    /// margin / padding insets in the layout pass.
    #[inline]
    pub fn deflated_by(self, s: Spacing) -> Self {
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
    pub const fn intersects(self, other: Self) -> bool {
        let a_max = self.max();
        let b_max = other.max();
        self.min.x < b_max.x
            && other.min.x < a_max.x
            && self.min.y < b_max.y
            && other.min.y < a_max.y
    }

    /// Strict axis-aligned intersection. `None` when the inputs don't overlap,
    /// touching edges included — the same answer [`Self::intersects`] gives as
    /// a bool, and the counterpart of the crate-internal `URect::intersect`.
    ///
    /// [`Self::clamp_to`] beside it is the saturating one, for a caller that
    /// wants a rect either way. The pair is named the same on both rectangles
    /// so that reading one does not teach the wrong thing about the other.
    #[inline]
    pub const fn intersect(self, other: Self) -> Option<Self> {
        let clamped = self.clamp_to(other);
        if clamped.size.w > 0.0 && clamped.size.h > 0.0 {
            Some(clamped)
        } else {
            None
        }
    }

    /// Saturating intersection: clamps `self` to fit inside `bounds`, giving a
    /// possibly zero-sized rect rather than nothing at all.
    ///
    /// The counterpart of the crate-internal `URect::clamp_to`, and what this
    /// method was called `intersect` for before there was a strict one to tell
    /// it apart from.
    #[inline]
    pub const fn clamp_to(self, bounds: Self) -> Self {
        let (a, b) = (self.max(), bounds.max());
        let min = Vec2::new(self.min.x.max(bounds.min.x), self.min.y.max(bounds.min.y));
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
    pub const fn union(self, other: Self) -> Self {
        if self.is_paint_empty() {
            return other;
        }
        if other.is_paint_empty() {
            return self;
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
    pub fn scaled_by(self, scale: f32, snap: bool) -> Self {
        // Scalar lanes because glam's `Vec2` ops aren't `const fn`.
        let m = self.max();
        let mut min = Vec2::new(self.min.x * scale, self.min.y * scale);
        let mut max = Vec2::new(m.x * scale, m.y * scale);
        if snap {
            min = Vec2::new(min.x.fast_round(), min.y.fast_round());
            max = Vec2::new(max.x.fast_round(), max.y.fast_round());
        }
        Self {
            min,
            size: Size::new((max.x - min.x).max(0.0), (max.y - min.y).max(0.0)),
        }
    }
}

impl NanCheck for Rect {
    #[inline]
    fn has_nan(&self) -> bool {
        Rect::has_nan(*self)
    }
}

#[cfg(test)]
mod tests;
