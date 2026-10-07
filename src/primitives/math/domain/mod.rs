//! The scalar rules every public input is read through.
//!
//! *Validation* is about the value alone (finite, not negative); *coercion* is about the value against its context (inside a range, an index that exists).
//!
//! - A **validating kind** is an `is_*` predicate plus a same-named checker that returns its argument or panics, in release too. Run-time values go through the predicate or a coercing kind first.
//! - A **coercing kind** is one total `const fn` that never panics.
//!
//! | Kind | Rule | Outside it |
//! |---|---|---|
//! | [`offset`] | finite | panic |
//! | [`length`] | finite, `>= 0` | panic |
//! | [`extent`] | `>= 0`, `+inf` allowed | panic |
//! | [`gap`] | a length `<= 65504` (one f16 lane) | panic |
//! | [`positive`] | finite, `> 0` | panic |
//! | [`angle`] | finite | panic |
//! | [`color`] | every channel finite | panic |
//! | [`count`] | `>= 1` | panic |
//! | [`power_of_two_in`] | a power of two in `1..=max` | panic |
//! | [`f64::range`] | both ends finite | panic; the order is coerced |
//! | [`fraction`] | `0..=1` | clamped; non-finite is `0` |
//! | [`turn`] | `0..1` | wrapped; non-finite is `0` |
//! | [`index`] | `0..len` | clamped; no index when `len == 0` |
//!
//! Theme files are checked on load by the same predicates. [`EPS`] is the one epsilon for "can the eye resolve this". Everything is `const fn` because the setters that call it are. [`vec2`] and [`f64`](mod@f64) hold the per-axis and `f64` twins.

use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;

pub mod f64;
pub mod vec2;

/// Float comparisons at UI tolerance, below 8-bit color precision and sub-pixel resolution.
pub const EPS: f32 = 1.0e-4;

/// The largest [`gap`]: the largest finite f16, since containers pack gaps into half-precision lanes.
pub const MAX_GAP: f32 = F16x4::MAX_LANE;

pub(crate) const OFFSET_RULE: &str = "an offset must be finite";
pub(crate) const LENGTH_RULE: &str = "a length must be finite and not negative";
pub(crate) const EXTENT_RULE: &str = "an extent must not be negative or NaN";
pub(crate) const GAP_RULE: &str = "a gap must be finite, not negative, and at most 65504";
pub(crate) const POSITIVE_RULE: &str = "a positive value must be finite and above zero";
pub(crate) const ANGLE_RULE: &str = "an angle must be finite";
pub(crate) const FRACTION_RULE: &str = "a fraction must be in 0..=1";
pub(crate) const COLOR_RULE: &str = "a color must have finite channels";
pub(crate) const COUNT_RULE: &str = "a count must be at least 1";
pub(crate) const POWER_OF_TWO_RULE: &str =
    "the value must be a power of two no larger than its maximum";
pub(crate) const RANGE_RULE: &str = "a range must have finite ends";

/// True if `v` is within [`EPS`] of zero.
#[inline]
pub const fn is_approx_zero(v: f32) -> bool {
    v.abs() <= EPS
}

/// True if `a` and `b` are within [`EPS`] of each other; false if either is NaN.
#[inline]
pub const fn approx_eq(a: f32, b: f32) -> bool {
    is_approx_zero(a - b)
}

/// True if `v` would produce no visible paint as a magnitude (stroke width, alpha); NaN counts.
#[inline]
pub const fn is_invisible(v: f32) -> bool {
    v.is_nan() || v <= EPS
}

/// `n / d`, or zero when `d` carries no paintable magnitude (a negative `d` included, being a distance that came out backwards).
#[inline]
pub const fn share_of(n: f32, d: f32) -> f32 {
    if is_invisible(d) { 0.0 } else { n / d }
}

/// Where `pos` sits along a track of `extent` as a 0..1 share, for a centred object of width `band`. Usable travel is `extent - band`; none left yields zero. Unclamped, see [`fraction_or`].
#[inline]
pub const fn band_fraction(pos: f32, extent: f32, band: f32) -> f32 {
    share_of(pos - band * 0.5, extent - band)
}

/// True if `v` is an [`offset`]: finite.
#[inline]
pub const fn is_offset(v: f32) -> bool {
    v.is_finite()
}

/// `v`, which must be an *offset*: a signed distance, finite.
///
/// # Panics
///
/// Panics unless [`is_offset`]`(v)`.
#[inline]
#[track_caller]
pub const fn offset(v: f32) -> f32 {
    assert!(is_offset(v), "{}", OFFSET_RULE);
    v
}

/// True if `v` is a [`length`]: finite and not negative.
#[inline]
pub const fn is_length(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

/// `v`, which must be a *length*: a distance, finite and not negative. Zero is a length.
///
/// # Panics
///
/// Panics unless [`is_length`]`(v)`.
#[inline]
#[track_caller]
pub const fn length(v: f32) -> f32 {
    assert!(is_length(v), "{}", LENGTH_RULE);
    v
}

/// True if `v` is an [`extent`]: not negative, and not NaN.
#[inline]
pub const fn is_extent(v: f32) -> bool {
    v >= 0.0
}

/// `v`, which must be an *extent*: a length or `+inf`.
///
/// # Panics
///
/// Panics unless [`is_extent`]`(v)`.
#[inline]
#[track_caller]
pub const fn extent(v: f32) -> f32 {
    assert!(is_extent(v), "{}", EXTENT_RULE);
    v
}

/// True if `v` is a [`gap`]: a length that fits one f16 lane.
#[inline]
pub const fn is_gap(v: f32) -> bool {
    is_length(v) && v <= MAX_GAP
}

/// `v`, which must be a *gap*: a length of at most 65504, one f16 lane.
///
/// # Panics
///
/// Panics unless [`is_gap`]`(v)`.
#[inline]
#[track_caller]
pub const fn gap(v: f32) -> f32 {
    assert!(is_gap(v), "{}", GAP_RULE);
    v
}

/// True if `v` is [`positive`]: finite and above zero.
#[inline]
pub const fn is_positive(v: f32) -> bool {
    v.is_finite() && v > 0.0
}

/// `v`, which must be *positive*: finite and above zero.
///
/// # Panics
///
/// Panics unless [`is_positive`]`(v)`.
#[inline]
#[track_caller]
pub const fn positive(v: f32) -> f32 {
    assert!(is_positive(v), "{}", POSITIVE_RULE);
    v
}

/// True if `v` is an [`angle`]: finite.
#[inline]
pub const fn is_angle(v: f32) -> bool {
    v.is_finite()
}

/// `v`, which must be an *angle*: finite, in radians.
///
/// # Panics
///
/// Panics unless [`is_angle`]`(v)`.
#[inline]
#[track_caller]
pub const fn angle(v: f32) -> f32 {
    assert!(is_angle(v), "{}", ANGLE_RULE);
    v
}

/// True if every channel of `c` is finite.
#[inline]
pub const fn is_color(c: RgbaF32) -> bool {
    c.r.is_finite() && c.g.is_finite() && c.b.is_finite() && c.a.is_finite()
}

/// `c`, which must be a *color*: every channel finite. Above `1` is valid (HDR, tween overshoot).
///
/// # Panics
///
/// Panics unless [`is_color`]`(c)`.
#[inline]
#[track_caller]
pub const fn color(c: RgbaF32) -> RgbaF32 {
    assert!(is_color(c), "{}", COLOR_RULE);
    c
}

/// True if `n` is a [`count`]: at least 1.
#[inline]
pub const fn is_count(n: u32) -> bool {
    n >= 1
}

/// `n`, which must be a *count*: at least 1.
///
/// # Panics
///
/// Panics unless [`is_count`]`(n)`.
#[inline]
#[track_caller]
pub const fn count(n: u32) -> u32 {
    assert!(is_count(n), "{}", COUNT_RULE);
    n
}

/// True if `n` is a power of two in `1..=max`.
#[inline]
pub const fn is_power_of_two_in(n: u32, max: u32) -> bool {
    n.is_power_of_two() && n <= max
}

/// `n`, which must be a power of two in `1..=max`.
///
/// # Panics
///
/// Panics unless [`is_power_of_two_in`]`(n, max)`.
#[inline]
#[track_caller]
pub const fn power_of_two_in(n: u32, max: u32) -> u32 {
    assert!(is_power_of_two_in(n, max), "{}", POWER_OF_TWO_RULE);
    n
}

/// True if `v` is a *fraction* as a file states one: in `0..=1`.
#[inline]
pub const fn is_fraction(v: f32) -> bool {
    v >= 0.0 && v <= 1.0
}

/// `v` as a *fraction*: clamped into `0..=1`, `0` when not finite.
#[inline]
pub const fn fraction(v: f32) -> f32 {
    fraction_or(v, 0.0)
}

/// `v` clamped into `0..=1`, or `fallback` where it is not finite (NaN survives `f32::clamp`, an infinity would clamp to an unmeant end). The fallback is the caller's: unknown progress is empty, an unknown split is centred.
#[inline]
pub const fn fraction_or(v: f32, fallback: f32) -> f32 {
    debug_assert!(
        is_fraction(fallback),
        "a fraction's fallback must itself be in 0..=1",
    );
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

/// `v` as a *turn*: wrapped into `0..1`, `0` when not finite. A hue is a turn.
#[inline]
pub const fn turn(v: f32) -> f32 {
    let t = v - v.floor();
    // A hair under zero wraps to `1 - ε`, which rounds to `1.0`.
    if t.is_nan() || t >= 1.0 { 0.0 } else { t }
}

/// `i` as an *index* into `len` items: clamped to the last, `None` when empty. Display only; the caller's index is untouched.
#[inline]
pub const fn index(i: usize, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else if i < len {
        Some(i)
    } else {
        Some(len - 1)
    }
}

/// A theme length floored at `min`, a widget design rule rather than validation.
#[inline]
pub const fn length_at_least(v: f32, min: f32) -> f32 {
    debug_assert!(is_length(min), "a length's floor must itself be a length");
    v.max(min)
}

#[cfg(test)]
pub(crate) mod internals {
    /// Asserts `actual` is within `tol` of `expected`; `why` names the inexactness.
    #[track_caller]
    pub(crate) fn assert_close(
        actual: impl Into<f64>,
        expected: impl Into<f64>,
        tol: f64,
        why: &str,
    ) {
        assert!(
            tol > 0.0 && !why.is_empty(),
            "a tolerance must be positive and carry its reason"
        );
        let (actual, expected) = (actual.into(), expected.into());
        assert!(
            (actual - expected).abs() <= tol,
            "{actual} is not within {tol} of {expected} ({why})",
        );
    }
}

#[cfg(test)]
mod tests;
