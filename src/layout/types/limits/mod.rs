//! What layout treats as a usable bound: the screens every measured lower
//! bound, upper bound and gap passes before the pass math trusts it.
//!
//! Named predicates rather than an assert per site, so "a bound layout can
//! work with" has one definition and a debug build checks the pair in one
//! call.

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

#[inline]
pub(crate) fn debug_assert_valid_bounds(min_size: Size, max_size: Size) {
    // Builder setters run per widget per frame, so validation compiles out in release.
    debug_assert!(
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
