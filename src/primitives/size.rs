use crate::primitives::{approx, num::Num};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl std::hash::Hash for Size {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        approx::hash_size(*self, state);
    }
}

impl Size {
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    pub const INF: Self = Self {
        w: f32::INFINITY,
        h: f32::INFINITY,
    };

    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// True if both axes are positive infinity. Distinct from
    /// `f32::is_infinite` (which also accepts `-INFINITY`) so callers using
    /// this as a "no upper bound" sentinel can't be tripped by negative
    /// infinity or NaN.
    pub const fn is_inf(self) -> bool {
        self.w == f32::INFINITY && self.h == f32::INFINITY
    }

    /// True if both axes are within `EPS` of zero — i.e. this size
    /// is approximately `Size::ZERO`. Strict (both-axis) semantic to
    /// match the crate's scalar `approx_zero` predicate.
    /// For "paints no pixels" use [`Self::is_paint_empty`] —
    /// different (looser) predicate.
    pub const fn approx_zero(self) -> bool {
        approx::approx_zero(self.w) && approx::approx_zero(self.h)
    }

    /// True when either axis is at or below `EPS` (including NaN /
    /// negative from degenerate construction). The shared "paints no
    /// pixels" predicate — call from any gate that wants to drop
    /// zero-extent geometry before emit / cache work runs.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        approx::noop_f32(self.w) || approx::noop_f32(self.h)
    }

    pub const fn min(self, other: Self) -> Self {
        Self {
            w: self.w.min(other.w),
            h: self.h.min(other.h),
        }
    }
    pub const fn max(self, other: Self) -> Self {
        Self {
            w: self.w.max(other.w),
            h: self.h.max(other.h),
        }
    }
}

impl<T: Num> From<T> for Size {
    fn from(v: T) -> Self {
        let v = v.as_f32();
        Self { w: v, h: v }
    }
}

impl<W: Num, H: Num> From<(W, H)> for Size {
    fn from((w, h): (W, H)) -> Self {
        Self {
            w: w.as_f32(),
            h: h.as_f32(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::size::Size;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_value(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn min_and_max_are_per_axis() {
        let a = Size::new(1.0, 8.0);
        let b = Size::new(4.0, 2.0);
        assert_eq!(a.min(b), Size::new(1.0, 2.0));
        assert_eq!(a.max(b), Size::new(4.0, 8.0));
    }

    #[test]
    fn min_and_max_ignore_nan_operand() {
        let nan = Size::new(f32::NAN, f32::NAN);
        let real = Size::new(3.0, 5.0);
        // `f32::min`/`max` ignore NaN when the other operand is a real
        // number — matches every other f32-pair reduction in the crate
        // (e.g. `Rect::union`/`intersect`).
        assert_eq!(real.min(nan), real);
        assert_eq!(real.max(nan), real);
    }

    #[test]
    fn equal_signed_zero_sizes_have_equal_hashes() {
        let positive = Size::new(0.0, 0.0);
        let negative = Size::new(-0.0, -0.0);

        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));
    }
}
