use crate::primitives::rect::Rect;
use glam::UVec2;

/// Axis-aligned rectangle in physical pixels (`u32`). Used for scissors,
/// glyph clip bounds, viewport extents — anywhere the renderer hands
/// integer pixel rects to the GPU. Logical-px rects use [`Rect`], whose
/// shape and method names this mirrors so that reading one teaches the
/// other.
///
/// Origin + extent, like [`Rect`] and for the same reason — layout and the
/// GPU both produce sizes — which also round-trips with wgpu's
/// `set_scissor_rect(x, y, w, h)` without arithmetic, since the four `u32`s
/// land in that order.
///
/// The extent is a [`UVec2`] where the float rect has a named
/// [`Size`](crate::primitives::size::Size): that type is a float extent to
/// its bones — approximate zero, infinity, NaN — and an integer one would
/// share none of it. The cost is `size.x` for a width where the float rect
/// says `size.w`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct URect {
    /// Top-left corner.
    pub min: UVec2,
    /// Extent from [`Self::min`]. The bottom-right corner is [`Self::max`].
    pub size: UVec2,
}

impl std::hash::Hash for URect {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl URect {
    /// Origin at `(0, 0)` with zero extent — [`Rect::ZERO`]'s counterpart, and
    /// the same value [`Default`] gives.
    pub(crate) const ZERO: Self = Self::new(0, 0, 0, 0);

    /// A rect from its top-left corner and extent.
    #[inline]
    pub(crate) const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            min: UVec2::new(x, y),
            size: UVec2::new(w, h),
        }
    }

    /// A rect from two corners, the max exclusive — [`Rect::from_min_max`]'s
    /// counterpart.
    ///
    /// Saturating where the float rect debug-asserts: there is no NaN here to
    /// make room for, and an inverted pair has an answer that is plainly right
    /// — the empty rect at `min` — where in floats it would have to be told
    /// apart from a legitimate zero.
    #[inline]
    pub(crate) const fn from_min_max(min: UVec2, max: UVec2) -> Self {
        Self {
            min,
            size: UVec2::new(max.x.saturating_sub(min.x), max.y.saturating_sub(min.y)),
        }
    }

    /// The pixels `rect` touches: floor on the min, ceil on the max, so an
    /// unsnapped float rect expands outward to cover its own source rather
    /// than cutting inside it.
    ///
    /// Which way it rounds is the whole of what this decides, and it rounds
    /// outward because the callers are bounds: under-bounding feeds false
    /// negatives to overlap tracking and to culling, where over-bounding costs
    /// a wasted comparison. A rect that is not finite covers nothing — there is
    /// no set of pixels a NaN edge names.
    ///
    /// Not a [`From`] for that reason: rounding is a policy, and a conversion
    /// that picked one silently would be one no call site had to agree with.
    /// The widening direction has nothing to pick and *is* a `From`.
    #[inline]
    pub(crate) fn covering(rect: Rect) -> Self {
        let (min, max) = (rect.min, rect.max());
        if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
            return Self::ZERO;
        }
        Self::from_min_max(
            UVec2::new(min.x.max(0.0) as u32, min.y.max(0.0) as u32),
            UVec2::new(max.x.max(0.0).ceil() as u32, max.y.max(0.0).ceil() as u32),
        )
    }

    /// Bottom-right corner — exclusive, per the half-open convention
    /// [`Rect::max`] follows.
    #[inline]
    pub(crate) const fn max(self) -> UVec2 {
        UVec2::new(self.min.x + self.size.x, self.min.y + self.size.y)
    }

    /// True when this rect paints no pixels, which for whole pixels is an
    /// empty axis and nothing subtler — [`Rect::is_paint_empty`]'s counterpart,
    /// which has NaN and a tolerance to answer for as well.
    #[inline]
    pub(crate) const fn is_paint_empty(self) -> bool {
        self.size.x == 0 || self.size.y == 0
    }

    /// True if `self` and `other` overlap on both axes — strict, so touching
    /// edges do not count. The predicate [`Self::intersect`] answers `Some` for.
    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        let (a, b) = (self.max(), other.max());
        self.min.x < b.x && other.min.x < a.x && self.min.y < b.y && other.min.y < a.y
    }

    /// Strict axis-aligned intersection. `None` when the inputs don't overlap,
    /// touching edges included. Used by the damage-rendering backend to combine
    /// the per-frame damage scissor with each group's existing clip scissor.
    ///
    /// [`Self::clamp_to`] is the saturating one, for a caller that wants a rect
    /// either way. The pair is named the same on both rectangles.
    #[inline]
    pub(crate) const fn intersect(self, other: Self) -> Option<Self> {
        let clamped = self.clamp_to(other);
        if clamped.is_paint_empty() {
            None
        } else {
            Some(clamped)
        }
    }

    /// Saturating intersection: clamps `self` to fit inside `bounds`, giving a
    /// possibly zero-sized rect. Used by the composer's clip stack, where
    /// parent-child overlap is the common case and a zero-sized result means
    /// "skip this group".
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

    /// Smallest axis-aligned rect enclosing both. A rect that paints nothing
    /// acts as the identity, so callers can fold a [`Self::ZERO`]-seeded
    /// accumulator without a special first-node branch — the same contract
    /// [`Rect::union`] keeps.
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

/// The rest of the surface [`Rect`] has, so that the two rectangles are one
/// vocabulary rather than two overlapping ones — a caller reaching across from
/// the float rect finds what it went looking for.
///
/// Nothing calls these yet, which is the whole reason they are gathered here
/// rather than left among the methods that are: the parity is deliberate, and
/// the first of them to earn a caller should be moved up rather than have the
/// allowance quietly widened around it.
#[allow(dead_code)]
impl URect {
    /// Midpoint, truncated — the pixel the middle falls in rather than the
    /// middle itself, which on an odd extent is half a pixel further on.
    #[inline]
    pub(crate) const fn center(self) -> UVec2 {
        UVec2::new(self.min.x + self.size.x / 2, self.min.y + self.size.y / 2)
    }

    /// `width * height`, widened because a surface-sized rect squares past
    /// `u32` — 65 536² already does.
    #[inline]
    pub(crate) const fn area(self) -> u64 {
        self.size.x as u64 * self.size.y as u64
    }

    /// Half-open containment: the min edges are inside, the max edges are not,
    /// so tiled rects never both claim the same pixel.
    #[inline]
    pub(crate) const fn contains(self, p: UVec2) -> bool {
        let max = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < max.x && p.y < max.y
    }

    /// True when `self` fully encloses `other`. Equality on the far edges
    /// counts, so `r.contains_rect(r)` holds.
    #[inline]
    pub(crate) const fn contains_rect(self, other: Self) -> bool {
        let (a, b) = (self.max(), other.max());
        other.min.x >= self.min.x && other.min.y >= self.min.y && b.x <= a.x && b.y <= a.y
    }

    /// Outset by `amount` on every side, saturating at both ends — the origin
    /// cannot go below zero and the extent cannot wrap.
    #[inline]
    pub(crate) const fn inflated(self, amount: u32) -> Self {
        Self {
            min: UVec2::new(
                self.min.x.saturating_sub(amount),
                self.min.y.saturating_sub(amount),
            ),
            size: UVec2::new(
                self.size.x.saturating_add(2 * amount),
                self.size.y.saturating_add(2 * amount),
            ),
        }
    }
}

/// Widening is exact and has nothing to decide, so it is the direction that
/// gets a [`From`] — see [`URect::covering`] for the way back, which has to
/// pick a rounding and is named for it.
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

/// 8-byte half-precision variant of [`URect`]. Same physical-px
/// semantics, but components saturate at `u16::MAX` (65 535 px) —
/// enough for 8K surfaces and 4×-DPI 16K. Used where many rects
/// are stored in a hot Pod struct (e.g. `TextRun.bounds`); halving
/// the footprint shrinks the per-frame `Vec<TextRun>` and the
/// bytemuck hash input.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct URect16 {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl std::hash::Hash for URect16 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl URect16 {
    /// Saturating constructor from `u32` components.
    pub(crate) const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            x: sat_u16(x),
            y: sat_u16(y),
            w: sat_u16(w),
            h: sat_u16(h),
        }
    }

    /// Widen to [`URect`] for boundary code (wgpu `set_scissor_rect`,
    /// glyphon `TextBounds`).
    pub(crate) const fn to_urect(self) -> URect {
        URect::new(self.x as u32, self.y as u32, self.w as u32, self.h as u32)
    }
}

impl From<URect> for URect16 {
    #[inline]
    fn from(r: URect) -> Self {
        Self::new(r.min.x, r.min.y, r.size.x, r.size.y)
    }
}

impl From<URect16> for URect {
    #[inline]
    fn from(r: URect16) -> Self {
        r.to_urect()
    }
}

/// `Ord::min` and `Ord::max` are not callable from a `const fn`, and every
/// rectangle operation here wants one — so the branch is written once under a
/// name rather than four times inside each.
const fn smaller(a: u32, b: u32) -> u32 {
    if a < b { a } else { b }
}

const fn larger(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}

const fn sat_u16(v: u32) -> u16 {
    if v > u16::MAX as u32 {
        u16::MAX
    } else {
        v as u16
    }
}

#[cfg(test)]
mod tests;
