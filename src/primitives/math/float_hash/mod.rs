//! Feeding floats to a hasher under the crate's two tolerances.
//!
//! Comparison is public in [`domain`](crate::primitives::math::domain); this half is cache identity.

use crate::primitives::math::domain;
use glam::Vec2;
use std::hash::Hasher;

/// Equality-compatible bits: both signed zeros share one hash, all else exact.
#[inline]
pub(crate) const fn eq_bits(f: f32) -> u32 {
    if f == 0.0 { 0 } else { f.to_bits() }
}

/// Canonical bits for content caches: values within the zero tolerance share one pattern, every NaN one quiet NaN.
#[inline]
pub(crate) const fn canon_bits(f: f32) -> u32 {
    if f.is_nan() {
        f32::NAN.to_bits()
    } else if domain::is_approx_zero(f) {
        0u32
    } else {
        f.to_bits()
    }
}

/// Feed a value to a hasher under one of the two float tolerances.
///
/// - `hash_eq` is the `Hash` half of `Hash`/`PartialEq`: only signed zeros fold together.
/// - `hash_visual` is cache identity: sub-`EPS` magnitudes collapse to zero, every NaN to one.
pub(crate) trait FloatHash {
    /// Feed `self` under equality-compatible canonicalization.
    fn hash_eq<H: Hasher>(&self, state: &mut H);

    /// Feed `self` under visual canonicalization.
    fn hash_visual<H: Hasher>(&self, state: &mut H);
}

impl FloatHash for f32 {
    #[inline]
    fn hash_eq<H: Hasher>(&self, state: &mut H) {
        state.write_u32(eq_bits(*self));
    }

    #[inline]
    fn hash_visual<H: Hasher>(&self, state: &mut H) {
        state.write_u32(canon_bits(*self));
    }
}

/// Both lanes in one `write_u64`: one hasher round per point on a per-vertex path.
impl FloatHash for Vec2 {
    #[inline]
    fn hash_eq<H: Hasher>(&self, state: &mut H) {
        state.write_u64((u64::from(eq_bits(self.x)) << 32) | u64::from(eq_bits(self.y)));
    }

    #[inline]
    fn hash_visual<H: Hasher>(&self, state: &mut H) {
        state.write_u64((u64::from(canon_bits(self.x)) << 32) | u64::from(canon_bits(self.y)));
    }
}

#[cfg(test)]
mod tests;
