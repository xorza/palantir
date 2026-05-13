use super::num::Num;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl std::hash::Hash for Size {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
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
    /// match the [`super::approx::approx_zero`] free fn on `f32`.
    /// For "paints no pixels" use [`Self::is_paint_empty`] —
    /// different (looser) predicate.
    pub const fn approx_zero(self) -> bool {
        super::approx::approx_zero(self.w) && super::approx::approx_zero(self.h)
    }

    /// True when either axis is at or below `EPS` (including NaN /
    /// negative from degenerate construction). The shared "paints no
    /// pixels" predicate — call from any gate that wants to drop
    /// zero-extent geometry before emit / cache work runs.
    ///
    /// The negated-comparison form (`!(x > EPS)`) catches NaN
    /// (a forward compare against NaN is always false → negated is
    /// true) where `x <= EPS` wouldn't.
    #[inline]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub const fn is_paint_empty(self) -> bool {
        !(self.w > super::approx::EPS) || !(self.h > super::approx::EPS)
    }

    pub const fn min(self, other: Self) -> Self {
        Self {
            w: if self.w < other.w { self.w } else { other.w },
            h: if self.h < other.h { self.h } else { other.h },
        }
    }
    pub const fn max(self, other: Self) -> Self {
        Self {
            w: if self.w > other.w { self.w } else { other.w },
            h: if self.h > other.h { self.h } else { other.h },
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
