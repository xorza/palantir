//! The physical-pixel rectangle the backend works in (scissors, damage regions,
//! atlas slots), where a fraction of a pixel has no meaning.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::num::F32Px;
use glam::UVec2;
use std::hash;

/// Axis-aligned rectangle in physical pixels (`u32`), for scissors, glyph clip
/// bounds and viewport extents. Logical-px rects use [`Rect`], whose shape and
/// names this mirrors.
///
/// Origin + extent, like [`Rect`], which round-trips with wgpu's
/// `set_scissor_rect(x, y, w, h)` without arithmetic.
///
/// The extent is a [`UVec2`], not the float
/// [`Size`](crate::primitives::geometry::size::Size) (approximate zero,
/// infinity, NaN), so a width is `size.x`, not `size.w`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct URect {
    pub min: UVec2,
    /// Extent from [`Self::min`]. The bottom-right corner is [`Self::max`].
    pub size: UVec2,
}

impl hash::Hash for URect {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl URect {
    /// Origin at `(0, 0)` with zero extent: [`Rect::ZERO`]'s counterpart, equal to
    /// [`Default`].
    pub(crate) const ZERO: Self = Self::new(0, 0, 0, 0);

    /// A rect from its top-left corner and extent.
    #[inline]
    pub(crate) const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            min: UVec2::new(x, y),
            size: UVec2::new(w, h),
        }
    }

    /// A rect from two corners, the max exclusive: [`Rect::from_min_max`]'s
    /// counterpart. Saturating where the float rect debug-asserts: an inverted pair
    /// has a plain answer, the empty rect at `min`.
    #[inline]
    pub(crate) const fn from_min_max(min: UVec2, max: UVec2) -> Self {
        Self {
            min,
            size: UVec2::new(max.x.saturating_sub(min.x), max.y.saturating_sub(min.y)),
        }
    }

    /// The pixels `rect` touches: floor on the min, ceil on the max, so an
    /// unsnapped float rect expands outward to cover its source.
    ///
    /// Outward because callers are bounds: under-bounding gives false negatives to
    /// overlap tracking and culling, over-bounding costs a comparison. A non-finite
    /// rect covers nothing.
    ///
    /// Not a [`From`]: rounding is a policy. The widening direction has nothing to
    /// pick and *is* a `From`.
    #[inline]
    #[expect(
        clippy::cast_sign_loss,
        reason = "each bound is held at zero or above before the cast"
    )]
    pub(crate) fn covering(rect: Rect) -> Self {
        let (min, max) = (rect.min, rect.max());
        if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
            return Self::ZERO;
        }
        Self::from_min_max(
            UVec2::new(min.x.max(0.0) as u32, min.y.max(0.0) as u32),
            UVec2::new(max.x.max(0.0).ceil_px(), max.y.max(0.0).ceil_px()),
        )
    }

    /// Bottom-right corner, exclusive (half-open, as [`Rect::max`]).
    #[inline]
    pub(crate) const fn max(self) -> UVec2 {
        UVec2::new(self.min.x + self.size.x, self.min.y + self.size.y)
    }

    /// True when this rect paints no pixels: an empty axis ([`Rect::is_paint_empty`]'s
    /// counterpart, which also has NaN and a tolerance).
    #[inline]
    pub(crate) const fn is_paint_empty(self) -> bool {
        self.size.x == 0 || self.size.y == 0
    }

    /// True if `self` and `other` overlap on both axes, strictly (touching edges
    /// don't count); the predicate [`Self::intersect`] answers `Some` for.
    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        let (a, b) = (self.max(), other.max());
        self.min.x < b.x && other.min.x < a.x && self.min.y < b.y && other.min.y < a.y
    }

    /// Strict intersection; `None` when the inputs don't overlap, touching edges
    /// included. [`Self::clamp_to`] is the saturating one.
    #[inline]
    pub(crate) const fn intersect(self, other: Self) -> Option<Self> {
        let clamped = self.clamp_to(other);
        if clamped.is_paint_empty() {
            None
        } else {
            Some(clamped)
        }
    }

    /// Saturating intersection: clamps `self` inside `bounds`, possibly to a
    /// zero-sized rect (the composer's clip stack reads that as "skip this group").
    #[inline]
    pub(crate) const fn clamp_to(self, bounds: Self) -> Self {
        let (a, b) = (self.max(), bounds.max());
        Self::from_min_max(
            UVec2::new(
                larger(self.min.x, bounds.min.x),
                larger(self.min.y, bounds.min.y),
            ),
            UVec2::new(smaller(a.x, b.x), smaller(a.y, b.y)),
        )
    }

    /// Smallest axis-aligned rect enclosing both. A rect that paints nothing is the
    /// identity, so a [`Self::ZERO`]-seeded fold needs no first-node branch (as
    /// [`Rect::union`]).
    #[inline]
    pub(crate) const fn union(self, other: Self) -> Self {
        if self.is_paint_empty() {
            return other;
        }
        if other.is_paint_empty() {
            return self;
        }
        let (a, b) = (self.max(), other.max());
        Self::from_min_max(
            UVec2::new(
                smaller(self.min.x, other.min.x),
                smaller(self.min.y, other.min.y),
            ),
            UVec2::new(larger(a.x, b.x), larger(a.y, b.y)),
        )
    }
}

/// Widening is exact, so it is the direction that gets a [`From`]; see
/// `URect::covering` for the way back.
impl From<URect> for Rect {
    #[inline]
    fn from(r: URect) -> Self {
        Rect::new(
            r.min.x as f32,
            r.min.y as f32,
            r.size.x as f32,
            r.size.y as f32,
        )
    }
}

/// `Ord::min` and `Ord::max` aren't callable from a `const fn`, so the branch
/// is written once here.
const fn smaller(a: u32, b: u32) -> u32 {
    if a < b { a } else { b }
}

const fn larger(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}

#[cfg(test)]
mod tests;
