//! A `[min, max]` pair a caller may hand over reversed.

/// A `[min, max]` pair put in order, so a reversed pair still clamps as meant.
///
/// Ordering uses float `min`/`max`, which return the other operand for a NaN bound, keeping a degenerate pair usable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Limits<T> {
    pub(crate) lo: T,
    pub(crate) hi: T,
}

impl<T: MinMax> Limits<T> {
    #[inline]
    pub(crate) fn of(min: T, max: T) -> Self {
        Self {
            lo: min.min_of(max),
            hi: min.max_of(max),
        }
    }

    /// `value` pulled into the pair, via `min`/`max` rather than `clamp`, which asserts ordered bounds and trips on NaN.
    #[inline]
    pub(crate) fn clamp(self, value: T) -> T {
        value.max_of(self.lo).min_of(self.hi)
    }
}

/// The float `min`/`max` pair as a bound [`Limits`] is generic over; `f32` and `f64` have them only as inherent methods.
pub(crate) trait MinMax: Copy {
    fn min_of(self, other: Self) -> Self;
    fn max_of(self, other: Self) -> Self;
}

impl MinMax for f32 {
    #[inline]
    fn min_of(self, other: Self) -> Self {
        self.min(other)
    }

    #[inline]
    fn max_of(self, other: Self) -> Self {
        self.max(other)
    }
}

impl MinMax for f64 {
    #[inline]
    fn min_of(self, other: Self) -> Self {
        self.min(other)
    }

    #[inline]
    fn max_of(self, other: Self) -> Self {
        self.max(other)
    }
}

#[cfg(test)]
mod tests;
