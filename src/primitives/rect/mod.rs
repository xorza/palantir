use crate::primitives::{corners::Corners, size::Size, spacing::Spacing};
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
        state.write(bytemuck::bytes_of(self));
    }
}

impl Rect {
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        size: Size::ZERO,
    };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            size: Size::new(w, h),
        }
    }

    pub const fn max(&self) -> Vec2 {
        Vec2::new(self.min.x + self.size.w, self.min.y + self.size.h)
    }
    pub const fn center(&self) -> Vec2 {
        Vec2::new(
            self.min.x + self.size.w * 0.5,
            self.min.y + self.size.h * 0.5,
        )
    }
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

    pub const fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }

    /// True when `self` fully encloses `other`. Equality on the right
    /// edges counts (so `r.contains_rect(r)` is `true`). Used by the
    /// damage-region merge policy to drop rects already covered by a
    /// bigger one.
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
    pub fn inflated(&self, amount: f32) -> Self {
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
        const KAPPA: f32 = 1.0 - core::f32::consts::FRAC_1_SQRT_2;
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
        let w = self.size.w - (l + r);
        let h = self.size.h - (t + b);
        Self {
            min: Vec2::new(self.min.x + l, self.min.y + t),
            size: Size::new(if w < 0.0 { 0.0 } else { w }, if h < 0.0 { 0.0 } else { h }),
        }
    }

    /// True if `self` and `other` overlap on both axes (strict — touching
    /// edges don't count). Used by the encoder's damage-rect filter to
    /// decide whether a node's paint commands can be skipped.
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
    pub const fn intersect(&self, other: Self) -> Self {
        let min_x = if self.min.x > other.min.x {
            self.min.x
        } else {
            other.min.x
        };
        let min_y = if self.min.y > other.min.y {
            self.min.y
        } else {
            other.min.y
        };
        let a_max_x = self.min.x + self.size.w;
        let b_max_x = other.min.x + other.size.w;
        let max_x = if a_max_x < b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.min.y + self.size.h;
        let b_max_y = other.min.y + other.size.h;
        let max_y = if a_max_y < b_max_y { a_max_y } else { b_max_y };
        let w = max_x - min_x;
        let h = max_y - min_y;
        Self {
            min: Vec2::new(min_x, min_y),
            size: Size::new(if w < 0.0 { 0.0 } else { w }, if h < 0.0 { 0.0 } else { h }),
        }
    }

    /// Smallest axis-aligned rect enclosing both `self` and `other`. Used by
    /// damage-rect computation to union prev+curr rects of dirty nodes.
    /// A zero-sized rect at the origin (the `Default`) acts as the identity
    /// for accumulation: `Rect::ZERO.union(r) == r` only when `r`'s min is
    /// `(0,0)` — callers should fold over `Option<Rect>` to avoid biasing
    /// toward the origin.
    pub const fn union(&self, other: Self) -> Self {
        let min_x = if self.min.x < other.min.x {
            self.min.x
        } else {
            other.min.x
        };
        let min_y = if self.min.y < other.min.y {
            self.min.y
        } else {
            other.min.y
        };
        let a_max_x = self.min.x + self.size.w;
        let b_max_x = other.min.x + other.size.w;
        let max_x = if a_max_x > b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.min.y + self.size.h;
        let b_max_y = other.min.y + other.size.h;
        let max_y = if a_max_y > b_max_y { a_max_y } else { b_max_y };
        Self {
            min: Vec2::new(min_x, min_y),
            size: Size::new(max_x - min_x, max_y - min_y),
        }
    }

    /// Scale by `scale` and optionally snap edges to integer pixels. Used at
    /// the logical→physical-px boundary inside the renderer; snapping derives
    /// width/height from rounded edges (not from `size * scale`) to avoid
    /// creeping width drift across rows of identical rects.
    pub fn scaled_by(&self, scale: f32, snap: bool) -> Self {
        let mut left = self.min.x * scale;
        let mut top = self.min.y * scale;
        let mut right = (self.min.x + self.size.w) * scale;
        let mut bottom = (self.min.y + self.size.h) * scale;
        if snap {
            left = left.round();
            top = top.round();
            right = right.round();
            bottom = bottom.round();
        }
        Self {
            min: Vec2::new(left, top),
            size: Size::new((right - left).max(0.0), (bottom - top).max(0.0)),
        }
    }
}

#[cfg(test)]
mod tests;
