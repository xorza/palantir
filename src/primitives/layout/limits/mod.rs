//! What layout treats as a usable bound: the screens every measured lower
//! bound, upper bound and gap passes before the pass math trusts it.
//!
//! The scalar rules are the [`domain`] kinds — a lower bound is a length, an
//! upper bound an extent — and one call checks a whole pair.
//!
//! **Checked in release**, at one strictness for every bound the crate
//! takes — a node's, a grid track's, a gap's. The pass math downstream
//! does not survive a bad one quietly: an inverted or NaN pair reaches
//! `f32::clamp` in `AxisSlot::resolve` and `AxisPlacement::arrange`, which
//! asserts the same ordering unconditionally and reports it in std's
//! words, several passes from the setter that took the value. The cost is
//! a handful of compares in a builder setter the caller reached for
//! deliberately.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::packed::half_simd::F16x4;

/// A gap travels in one f16 lane.
pub(crate) const MAX_PACKED_GAP: f32 = F16x4::MAX_LANE;

/// # Panics
///
/// Panics unless both minimums are finite and non-negative, both maximums
/// are non-negative, and each minimum is at most its maximum. Positive
/// infinity is the unbounded maximum.
#[inline]
pub(crate) fn assert_valid_bounds(min_size: Size, max_size: Size) {
    assert!(
        domain::is_length(min_size.w)
            && domain::is_length(min_size.h)
            && domain::is_extent(max_size.w)
            && domain::is_extent(max_size.h)
            && min_size.w <= max_size.w
            && min_size.h <= max_size.h,
        "node minimums must be finite, bounds must be non-negative and ordered, and only \
         maximums may be infinite; got min_size {min_size:?}, max_size {max_size:?}",
    );
}

#[cfg(test)]
mod tests;
