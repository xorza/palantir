//! The [`domain`](super) rules for the `f64` values a [`Slider`](crate::Slider) or [`DragValue`](crate::DragValue) binds: twins of the scalar rules with the same message, and the slider's track range.

use crate::primitives::math::domain;
use std::ops::RangeInclusive;

/// True if `v` is [`positive`]: finite and above zero.
#[inline]
pub const fn is_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// `v`, which must be *positive* (a step, a speed).
///
/// # Panics
///
/// Panics unless [`is_positive`]`(v)`.
#[inline]
#[track_caller]
pub const fn positive(v: f64) -> f64 {
    assert!(is_positive(v), "{}", domain::POSITIVE_RULE);
    v
}

/// True if both ends of `r` are finite, in either order.
#[inline]
pub const fn is_range(r: &RangeInclusive<f64>) -> bool {
    r.start().is_finite() && r.end().is_finite()
}

/// `r` in ascending order, whose ends must be finite.
/// The ends are validated and the order coerced: a reversed range is ordinary data (e.g. from a file) meaning the same span.
///
/// # Panics
///
/// Panics unless [`is_range`]`(&r)`.
#[inline]
#[track_caller]
pub const fn range(r: RangeInclusive<f64>) -> RangeInclusive<f64> {
    assert!(is_range(&r), "{}", domain::RANGE_RULE);
    let (a, b) = (*r.start(), *r.end());
    RangeInclusive::new(a.min(b), a.max(b))
}
