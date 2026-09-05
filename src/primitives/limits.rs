//! A `[min, max]` pair a caller may hand over reversed.

/// A `[min, max]` pair put in order, so a caller may hand one over
/// reversed and still get the clamp it meant.
///
/// A widget takes its bounds from app code and cannot assert on their
/// order, so tolerating a reversed pair is a contract the slider and the
/// drag value both owe, not defensiveness.
///
/// Ordering goes through the float `min`/`max`, not through `<`: those
/// return the *other* operand for a NaN bound, which keeps a degenerate
/// pair usable rather than turning it into a clamp that panics.
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

    /// `value` pulled into the pair.
    ///
    /// Spelled as two `min`/`max` steps rather than the inherent
    /// `clamp`, which asserts its bounds are ordered — an assert this
    /// type exists to make unnecessary, and one a NaN bound trips.
    #[inline]
    pub(crate) fn clamp(self, value: T) -> T {
        value.max_of(self.lo).min_of(self.hi)
    }
}

/// The float `min`/`max` pair, as a bound [`Limits`] can be generic over.
///
/// `f32` and `f64` carry these as inherent methods rather than through a
/// trait, so a generic over both needs one — and the standard `Ord` is
/// not it, since neither type has it.
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
mod tests {
    use crate::primitives::limits::Limits;

    /// A reversed pair means the same range as the ordered one, and both
    /// clamp identically at the ends and inside.
    #[test]
    fn ordering_is_tolerated_and_clamping_matches() {
        let forward = Limits::of(0.0_f32, 10.0);
        let reversed = Limits::of(10.0_f32, 0.0);
        assert_eq!(forward, reversed);
        assert_eq!((forward.lo, forward.hi), (0.0, 10.0));
        for (value, want) in [
            (5.0, 5.0),
            (-1.0, 0.0),
            (11.0, 10.0),
            (0.0, 0.0),
            (10.0, 10.0),
        ] {
            assert_eq!(forward.clamp(value), want, "clamp({value})");
            assert_eq!(reversed.clamp(value), want, "reversed clamp({value})");
        }
    }

    /// A degenerate pair collapses to its single point, and an infinite
    /// one passes every finite value through — the unbounded drag value.
    #[test]
    fn degenerate_and_infinite_pairs_stay_usable() {
        assert_eq!(Limits::of(3.0_f64, 3.0).clamp(9.0), 3.0);
        let unbounded = Limits::of(f64::NEG_INFINITY, f64::INFINITY);
        assert_eq!(unbounded.clamp(-1.0e300), -1.0e300);
        assert_eq!(unbounded.clamp(1.0e300), 1.0e300);
    }

    /// A NaN bound reports the other end for both lanes, so the clamp
    /// still pins to a real number instead of tripping the inherent
    /// `clamp`'s ordered-bounds assert.
    #[test]
    fn a_nan_bound_falls_back_to_the_other_end() {
        let limits = Limits::of(f32::NAN, 4.0);
        assert_eq!((limits.lo, limits.hi), (4.0, 4.0));
        assert_eq!(limits.clamp(100.0), 4.0);
    }
}
