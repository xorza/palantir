use crate::primitives::{approx::noop_f32, num::Num};

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

/// Custom serde so an infinite axis ("unbounded" — e.g. a tooltip's
/// max height) survives formats with no infinity literal (Rhai, and
/// JSON which `serde_rhai` routes through). Each axis serializes as
/// `Option<f32>`: a finite value stays a plain number, a non-finite
/// one becomes `None` (`null` / Rhai `()`). On the way back `None`
/// restores `f32::INFINITY`. Finite sizes round-trip byte-identically
/// to the old `{ w, h }` form. NaN / -INFINITY collapse to
/// +INFINITY — neither is a meaningful `Size` value, and both read as
/// "unbounded" anyway.
impl serde::Serialize for Size {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let finite = |v: f32| v.is_finite().then_some(v);
        let mut st = s.serialize_struct("Size", 2)?;
        st.serialize_field("w", &finite(self.w))?;
        st.serialize_field("h", &finite(self.h))?;
        st.end()
    }
}

impl<'de> serde::Deserialize<'de> for Size {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            w: Option<f32>,
            h: Option<f32>,
        }
        let r = Raw::deserialize(d)?;
        Ok(Size::new(
            r.w.unwrap_or(f32::INFINITY),
            r.h.unwrap_or(f32::INFINITY),
        ))
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
    /// match the [`crate::primitives::approx::approx_zero`] free fn on `f32`.
    /// For "paints no pixels" use [`Self::is_paint_empty`] —
    /// different (looser) predicate.
    pub const fn approx_zero(self) -> bool {
        crate::primitives::approx::approx_zero(self.w)
            && crate::primitives::approx::approx_zero(self.h)
    }

    /// True when either axis is at or below `EPS` (including NaN /
    /// negative from degenerate construction). The shared "paints no
    /// pixels" predicate — call from any gate that wants to drop
    /// zero-extent geometry before emit / cache work runs.
    #[inline]
    pub const fn is_paint_empty(self) -> bool {
        noop_f32(self.w) || noop_f32(self.h)
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
}
