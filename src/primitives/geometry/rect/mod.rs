//! The logical-pixel rectangle every pass works in, plus the NaN-safe fold that derives one from points.

pub(crate) mod aabb;

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::float_hash::canon_bits;
use crate::primitives::math::nan::{self, NanCheck};
use crate::primitives::math::num::F32Px;
use core::f32::consts::FRAC_1_SQRT_2;
use glam::Vec2;
use std::hash;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
/// An axis-aligned rectangle in logical pixels, stored as origin + extent.
///
/// Half-open on both axes: [`Self::contains`] accepts the min edge and rejects the max, so adjacent rects tile without double-hitting. Hashing is approximate (`1e-4` tolerance).
#[must_use]
pub struct Rect {
    /// Top-left corner.
    pub min: Vec2,
    /// Extent from [`Self::min`]. The bottom-right corner is
    /// [`Self::max`].
    pub size: Size,
}

impl hash::Hash for Rect {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// Origin then extent, each packed by its own impl, so a rect and its `(Vec2, Size)` pair hash the same bytes.
impl FloatHash for Rect {
    #[inline]
    fn hash_eq<H: hash::Hasher>(&self, state: &mut H) {
        self.min.hash_eq(state);
        self.size.hash_eq(state);
    }

    #[inline]
    fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
        self.min.hash_visual(state);
        self.size.hash_visual(state);
    }
}

impl Rect {
    /// Panics unless every component is an [offset](crate::widget::domain::offset) (finite). A negative size is not refused: it paints nothing.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        vec2::offset(self.min);
        domain::offset(self.size.w);
        domain::offset(self.size.h);
    }

    /// Origin at `(0, 0)` with zero extent.
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        size: Size::ZERO,
    };

    /// The poisoned rect an [`Aabb`](aabb::Aabb) folds to after a NaN. [`Self::is_paint_empty`] reports it invisible.
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
    /// Debug-asserts that `min` is componentwise `<= max`, unless a corner is NaN.
    #[inline]
    pub const fn from_min_max(min: Vec2, max: Vec2) -> Self {
        // A NaN corner is expected under the AABB NaN contract (see [`Aabb`](aabb::Aabb)): it is carried into the bounds so the shape-level gate can name the shape. `max - min` keeps the NaN in `size`.
        debug_assert!(
            (min.x <= max.x && min.y <= max.y) || nan::vec2_has_nan(min) || nan::vec2_has_nan(max)
        );
        Self {
            min,
            size: Size::new(max.x - min.x, max.y - min.y),
        }
    }

    /// The four canonical lanes [`FloatHash::hash_visual`] writes, as data, for callers that hash them in one POD write.
    #[inline]
    pub(crate) const fn canon_lanes(self) -> [u32; 4] {
        [
            canon_bits(self.min.x),
            canon_bits(self.min.y),
            canon_bits(self.size.w),
            canon_bits(self.size.h),
        ]
    }

    /// Bottom-right corner, exclusive.
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

    /// True when this rect paints no pixels: an axis is `<= EPS`, NaN or negative. Defers to [`Size::is_paint_empty`].
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        self.size.is_paint_empty() || nan::vec2_has_nan(self.min)
    }

    /// True if any of the four lanes is NaN. `const`; the [`NanCheck`] impl delegates here.
    ///
    /// [`NanCheck`]: crate::primitives::math::nan::NanCheck
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        nan::vec2_has_nan(self.min) || self.size.has_nan()
    }

    /// Half-open containment: min edges inside, max edges outside.
    #[inline]
    pub const fn contains(self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// True when `self` fully encloses `other`; equal right edges count.
    #[inline]
    pub const fn contains_rect(self, other: Self) -> bool {
        let self_max = self.max();
        let other_max = other.max();
        other.min.x >= self.min.x
            && other.min.y >= self.min.y
            && other_max.x <= self_max.x
            && other_max.y <= self_max.y
    }

    /// Outset by `amount` on each side. Counterpart to [`Self::deflated`], which clamps where this does not.
    #[inline]
    pub const fn inflated(self, amount: f32) -> Self {
        Self {
            min: Vec2::new(self.min.x - amount, self.min.y - amount),
            size: Size::new(self.size.w + 2.0 * amount, self.size.h + 2.0 * amount),
        }
    }

    /// Inset by `amount` on each side, clamping the size at zero. Counterpart to [`Self::inflated`]; the uniform case of [`Self::deflated_by`].
    #[inline]
    pub const fn deflated(self, amount: f32) -> Self {
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
    pub(crate) const fn square_about(center: Vec2, half: f32) -> Self {
        Self {
            min: Vec2::new(center.x - half, center.y - half),
            size: Size::new(2.0 * half, 2.0 * half),
        }
    }

    /// Owner-local point a shape inside this rect spins about: its centre, with `min` no part of it.
    #[inline]
    pub(crate) const fn spin_pivot(self) -> Vec2 {
        Vec2::new(self.size.w * 0.5, self.size.h * 0.5)
    }

    /// Distance from `pivot` to this rect's farthest corner: the radius of the disc it sweeps.
    #[inline]
    pub(crate) fn spun_radius(self, pivot: Vec2) -> f32 {
        (self.min - pivot)
            .abs()
            .max((self.max() - pivot).abs())
            .length()
    }

    /// The square this rect covers at every rotation about `pivot`; angle-free, so composer culling and cascade damage agree.
    #[inline]
    pub(crate) fn spun_cover(self, pivot: Vec2) -> Self {
        Self::square_about(pivot, self.spun_radius(pivot))
    }

    /// Largest axis-aligned rect inside `self` when it bounds a rounded-rect paint with the given radii; each side is inset by `max(adjacent_radii) * (1 - 1/√2)`.
    #[inline]
    pub fn inscribed_for_corners(self, corners: Corners) -> Self {
        // `1 - 1/√2 ≈ 0.2929`: inward offset to a quarter arc's 45° point per unit radius.
        const KAPPA: f32 = 1.0 - FRAC_1_SQRT_2;

        if corners.is_approx_zero() {
            return self;
        }
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

    /// Outset by `s` on each side; undoes a non-clamped [`Self::deflated_by`].
    #[inline]
    pub fn inflated_by(self, s: Spacing) -> Self {
        let [l, t, r, b] = s.as_array();
        Self {
            min: Vec2::new(self.min.x - l, self.min.y - t),
            size: Size::new(self.size.w + (l + r), self.size.h + (t + b)),
        }
    }

    /// Inset by `s` on each side, clamping the size at zero.
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

    /// True if `self` and `other` overlap on both axes; touching edges don't count.
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        let a_max = self.max();
        let b_max = other.max();
        self.min.x < b_max.x
            && other.min.x < a_max.x
            && self.min.y < b_max.y
            && other.min.y < a_max.y
    }

    /// Strict intersection: `None` when the inputs don't overlap, touching included. [`Self::clamp_to`] is the saturating one.
    #[inline]
    pub const fn intersect(self, other: Self) -> Option<Self> {
        let clamped = self.clamp_to(other);
        if clamped.size.w > 0.0 && clamped.size.h > 0.0 {
            Some(clamped)
        } else {
            None
        }
    }

    /// Saturating intersection: clamps `self` into `bounds`, possibly to zero size. Neither operand may hold a NaN, or the clip silently stops clipping.
    #[inline]
    pub const fn clamp_to(self, bounds: Self) -> Self {
        debug_assert!(
            !self.has_nan() && !bounds.has_nan(),
            "Rect::clamp_to with a NaN operand"
        );
        let (a, b) = (self.max(), bounds.max());
        let min = Vec2::new(self.min.x.max(bounds.min.x), self.min.y.max(bounds.min.y));
        let max = Vec2::new(a.x.min(b.x), a.y.min(b.y));
        Self {
            min,
            size: Size::new((max.x - min.x).max(0.0), (max.y - min.y).max(0.0)),
        }
    }

    /// Smallest rect enclosing both. A paint-empty operand (NaN included) is the identity, so a `Rect::ZERO`-seeded fold needs no first-node branch.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        if self.is_paint_empty() {
            return other;
        }
        if other.is_paint_empty() {
            return self;
        }
        let (a, b) = (self.max(), other.max());
        let min = Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y));
        let max = Vec2::new(a.x.max(b.x), a.y.max(b.y));
        Self {
            min,
            size: Size::new(max.x - min.x, max.y - min.y),
        }
    }

    /// Scale by `scale` and optionally snap edges to integer pixels, deriving size from the rounded edges to avoid width drift.
    #[inline]
    pub(crate) fn scaled_by(self, scale: f32, snap: bool) -> Self {
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
