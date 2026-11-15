//! The [`domain`](super) rules for the `f64` values a [`Slider`](crate::Slider) or [`DragValue`](crate::DragValue) binds: twins of the scalar rules with the same message, and the slider's track range.

use crate::primitives::math::domain;
use std::ops::RangeInclusive;

/// True if `v` is [`positive`]: finite and above zero.
#[inline]
pub(crate) const fn is_positive(v: f64) -> bool {
    v > 0.0 && v <= f64::MAX
}

/// `v`, which must be *positive* (a step, a speed).
///
/// # Panics
///
/// Panics unless [`is_positive`]`(v)`.
#[inline]
#[track_caller]
pub(crate) const fn positive(v: f64) -> f64 {
    assert!(is_positive(v), "{}", domain::POSITIVE_RULE);
    v
}

/// True if both ends of `r` are finite, in either order.
#[inline]
pub(crate) const fn is_range(r: &RangeInclusive<f64>) -> bool {
    r.start().is_finite() && r.end().is_finite()
}

/// `r`, whose ends must be finite. The order is kept, as the direction is the meaning: a
/// [`Slider`](crate::Slider) over `10.0..=0.0` runs from right to left.
///
/// # Panics
///
/// Panics unless [`is_range`]`(&r)`.
#[inline]
#[track_caller]
pub(crate) const fn range(r: RangeInclusive<f64>) -> RangeInclusive<f64> {
    assert!(is_range(&r), "{}", domain::RANGE_RULE);
    r
}

/// True if neither end of `r` is NaN; an end may be infinite, in either order.
#[inline]
pub(crate) const fn is_bounds(r: &RangeInclusive<f64>) -> bool {
    !r.start().is_nan() && !r.end().is_nan()
}

/// `r`'s ends, which must not be NaN, ordered low to high, as a clamp reads them: a
/// [`DragValue`](crate::DragValue)'s range, which an infinite end leaves open.
///
/// # Panics
///
/// Panics unless [`is_bounds`]`(&r)`.
#[inline]
#[track_caller]
pub(crate) const fn bounds(r: RangeInclusive<f64>) -> RangeInclusive<f64> {
    assert!(is_bounds(&r), "{}", domain::BOUNDS_RULE);
    let (start, end) = (*r.start(), *r.end());
    start.min(end)..=start.max(end)
}
