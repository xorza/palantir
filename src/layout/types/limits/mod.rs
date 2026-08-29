//! What layout treats as a usable bound: the screens every measured lower
//! bound, upper bound and gap passes before the pass math trusts it.
//!
//! Named predicates rather than an assert per site, so "a bound layout can
//! work with" has one definition and one call checks a whole pair.
//!
//! **Checked in release**, at one strictness for every bound the crate
//! takes — a node's, a grid track's, a gap's. The pass math downstream
//! does not survive a bad one quietly: an inverted or NaN pair reaches
//! `f32::clamp` in `AxisCtx::resolve` and `AxisPlacement::arrange`, which
//! asserts the same ordering unconditionally and reports it in std's
//! words, several passes from the setter that took the value. The cost is
//! a handful of compares in a builder setter the caller reached for
//! deliberately.

use crate::primitives::size::Size;

pub(crate) const MAX_PACKED_GAP: f32 = 65_504.0;

#[inline]
pub(crate) const fn valid_lower_bound(value: f32) -> bool {
    value >= 0.0 && value < f32::INFINITY
}

#[inline]
pub(crate) const fn valid_upper_bound(value: f32) -> bool {
    value >= 0.0
}

#[inline]
pub(crate) const fn valid_gap(value: f32) -> bool {
    valid_lower_bound(value)
}

#[inline]
pub(crate) const fn valid_packed_gap(value: f32) -> bool {
    valid_gap(value) && value <= MAX_PACKED_GAP
}

/// # Panics
///
/// Panics unless both minimums are finite and non-negative, both maximums
/// are non-negative, and each minimum is at most its maximum. Positive
/// infinity is the unbounded maximum.
#[inline]
pub(crate) fn assert_valid_bounds(min_size: Size, max_size: Size) {
    assert!(
        valid_lower_bound(min_size.w)
            && valid_lower_bound(min_size.h)
            && valid_upper_bound(max_size.w)
            && valid_upper_bound(max_size.h)
            && min_size.w <= max_size.w
            && min_size.h <= max_size.h,
        "node minimums must be finite, bounds must be non-negative and ordered, and only \
         maximums may be infinite; got min_size {min_size:?}, max_size {max_size:?}",
    );
}

#[cfg(test)]
mod tests;
