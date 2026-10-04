//! The twins of the [`domain`](super) rules for the `f64` values a
//! [`Slider`](crate::Slider) or a [`DragValue`](crate::DragValue) binds,
//! each the scalar rule on an `f64`, with the same message.

use crate::primitives::math::domain;

/// True if `v` is [`positive`]: finite and above zero.
#[inline]
pub const fn is_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// `v`, which must be *positive*: finite and above zero — a step, a speed.
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
