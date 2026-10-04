//! The scalar rules every public input is read through: what a value of
//! each kind may be, and what the eye can tell apart.
//!
//! # Validation and coercion
//!
//! An input is asked two separate questions, as WPF asks them of a
//! property value. *Validation* is about the value alone — finite, not
//! negative, a power of two. *Coercion* is about the value against its
//! context — inside a range, an index that exists.
//!
//! - A **validating kind** is two `const fn`s: an `is_*` predicate, and a
//!   checker of the same name that returns its argument or panics with the
//!   kind's rule. A builder setter stores what the checker returns, so a
//!   bad value panics on the line that passed it (`#[track_caller]`), in
//!   release as well as debug: a wrong value is never drawn quietly. A
//!   value computed at run time — a `0 / 0` thickness — goes through the
//!   predicate, or a coercing kind, before it reaches such a setter.
//! - A **coercing kind** is one total `const fn`. It never panics: its
//!   context is data, which can shrink or arrive from a file, so an
//!   out-of-range value is pulled into range in a documented way.
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
//! Theme files are checked on load by the same predicates, so a file and a
//! call site cannot disagree about a value.
//!
//! # Tolerance
//!
//! One epsilon ([`EPS`]) answers "can the eye resolve this", and the
//! predicates are that comparison under a name: is this zero, does this
//! paint, do these coincide, what share is this. A widget outside this
//! crate asks the same questions and must reach the same epsilon — two
//! tolerances for one screen is how a rule lands on one side of a seam and
//! its handle on the other.
//!
//! Every function is a `const fn` on `f32` rather than a method, because a
//! trait method cannot be `const` and the setters that call these are.
//! [`vec2`] holds the per-axis twins a two-axis widget needs, and [`f64`](mod@f64)
//! the twins for the `f64` values a slider or a drag value binds.

use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;

pub mod f64;
pub mod vec2;

/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
pub const EPS: f32 = 1.0e-4;

/// The largest [`gap`]: 65504, the largest finite f16, because a container
/// packs its gaps into half-precision lanes.
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

/// True if `a` and `b` are within [`EPS`] of each other. False when either
/// is NaN.
#[inline]
pub const fn approx_eq(a: f32, b: f32) -> bool {
    is_approx_zero(a - b)
}

/// True if `v` would produce no visible paint when used as a magnitude
/// (stroke width, alpha, etc.).
///
/// One comparison covers near-zero positives, exact zero and every
/// negative; NaN, which compares false against everything, is the other
/// branch.
///
/// "Does this paint anything?" is asked at two tiers, and they compose.
/// `is_paint_empty` is the geometry half — does this `Size` / `Rect`
/// cover any pixels at all — and bottoms out here. `is_noop` is the whole
/// question, and a type that carries both geometry and paint answers it by
/// asking the first and then testing its ink.
#[inline]
pub const fn is_invisible(v: f32) -> bool {
    v.is_nan() || v <= EPS
}

/// `n / d`, or zero when `d` carries no paintable magnitude.
///
/// The one answer to "what share of `d` is `n`" for a `d` that geometry
/// can legitimately collapse — a scroll range with nothing to scroll, a
/// bar whose thumb fills its track, a line height with no font behind
/// it, a slider rail narrower than its own knob. Flooring the divisor at
/// a tolerance instead returns an enormous number for a quantity every
/// caller then reads as a fraction: a wrong answer stated confidently.
///
/// The gate is [`is_invisible`], so a *negative* `d` is degenerate too and
/// not merely a sign flip. Every `d` this divides is a distance, and one
/// that came out backwards has no share to report any more than a zero
/// one does.
#[inline]
pub const fn share_of(n: f32, d: f32) -> f32 {
    if is_invisible(d) { 0.0 } else { n / d }
}

/// Where `pos` sits along a track of `extent` that reserves `band` to a
/// centred thing the pointer drags, as a 0..1 share.
///
/// A slider's knob and a splitter's rule are the same placement problem: a
/// fixed-width object whose *centre* follows the pointer, so half the band
/// comes off each end before the division and the usable travel is
/// `extent - band`. A track with no travel left has no share to report and
/// yields zero.
///
/// The result is unclamped — a pointer outside the track reports outside
/// `0..1`, and each caller pins it with the bounds it enforces, which is
/// what [`fraction_or`] is for.
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

/// `v`, which must be a *length*: a distance, finite and not negative.
///
/// Zero is a length: a zero-width stroke or a zero-size font paints
/// nothing, which is what it says.
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

/// `v`, which must be an *extent*: a length or `+inf`, the unbounded
/// maximum.
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

/// `v`, which must be a *gap*: a length of at most 65504, the largest f16,
/// because a container packs its gaps into half-precision lanes.
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

/// `v`, which must be *positive*: finite and above zero — a scale, a rate,
/// a step, a weight.
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

/// `v`, which must be an *angle*: finite, in radians, any number of turns.
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

/// `c`, which must be a *color*: every channel finite. A channel above `1`
/// is valid — HDR values and tween overshoot reach it.
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

/// `n`, which must be a *count*: at least 1. A count of zero is a caller
/// bug — a grid cell that spans nothing — not data to coerce.
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

/// `n`, which must be a power of two in `1..=max` — a resolution divisor.
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

/// `v` as a *fraction*: clamped into `0..=1`, and `0` when it is not
/// finite. [`fraction_or`] with `0` as the fallback.
#[inline]
pub const fn fraction(v: f32) -> f32 {
    fraction_or(v, 0.0)
}

/// `v` as a share of something — clamped into `0..=1`, or `fallback` where
/// it names no share at all.
///
/// The clamp alone is not the rule. `f32::clamp` answers NaN for NaN, and
/// every consumer of a share turns it into a `Fill` weight, a track extent,
/// or a seam position — each of which rejects one. An infinity clamps to
/// an *end*, which states a share the caller never meant. Both non-finite
/// cases are "no share", so both take the fallback.
///
/// `fallback` is the caller's, because "no share" resolves differently:
/// unknown progress is empty, an unknown split is centred.
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

/// `v` as a *turn*: wrapped into `0..1`, so `1.25` and `-0.75` are both
/// `0.25`, and `0` when it is not finite. A hue is a turn.
#[inline]
pub const fn turn(v: f32) -> f32 {
    let t = v - v.floor();
    // A negative value a hair under zero wraps to `1 - ε`, which rounds to
    // `1.0` — the same hue as zero, and outside the half-open range.
    if t.is_nan() || t >= 1.0 { 0.0 } else { t }
}

/// `i` as an *index* into `len` items: clamped to the last one, and `None`
/// when there are none.
///
/// For display only. A stale index — an option list that shrank under it —
/// shows the last item, and the caller's own index stays as it was until
/// the user picks.
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

/// A length read out of a theme, floored at `min`.
///
/// The floor is a widget design rule, not validation: a theme file's
/// lengths are checked where they are loaded, so what arrives here is
/// already a length. `min` is the widget's, not the type's. A rule the
/// theme sets to zero is a rule the app wanted invisible, while a grab bar
/// or a spinner that thin cannot be grabbed or seen at all.
#[inline]
pub const fn length_at_least(v: f32, min: f32) -> f32 {
    debug_assert!(is_length(min), "a length's floor must itself be a length");
    v.max(min)
}

#[cfg(test)]
pub(crate) mod internals {
    /// Assert `actual` is within `tol` of `expected`. `why` names what
    /// makes the value inexact — a test that cannot say should use
    /// `assert_eq!`.
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
