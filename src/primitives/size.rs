use super::Num;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
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
