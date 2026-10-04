//! Feeding floats to a hasher under the crate's two tolerances.
//!
//! The comparison half of the same question — is this zero, do these
//! coincide — is public in [`domain`](crate::primitives::math::domain). This
//! half stays inside: it is cache identity, not geometry.

use crate::primitives::math::domain;
use glam::Vec2;
use std::hash::Hasher;

/// Equality-compatible bits for public `Hash` implementations. Rust float
/// equality treats both signed zeros as equal, so they must share one hash;
/// every other value retains its exact representation.
#[inline]
pub(crate) const fn eq_bits(f: f32) -> u32 {
    if f == 0.0 { 0 } else { f.to_bits() }
}

/// Canonicalize an `f32` at visual content-cache boundaries: collapse values
/// visually indistinguishable from zero to one bit pattern and every NaN to
/// one quiet NaN. Values outside the zero tolerance retain their exact bits.
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

/// Feeding a value to a hasher under one of the two float tolerances this
/// crate keeps apart.
///
/// - [`hash_eq`](Self::hash_eq) is the `Hash` half of `Hash`/`PartialEq`
///   agreement: only the signed zeros are folded together, because only
///   they compare equal.
/// - [`hash_visual`](Self::hash_visual) is content-cache identity: a
///   difference the eye cannot resolve must not split a cache key, so
///   sub-`EPS` magnitudes collapse to one zero and every NaN to one
///   quiet NaN.
///
/// A trait rather than a `hash_visual_{f32,vec2,size,rect}` family: the
/// suffix was type dispatch spelled by hand, and it kept a type's second
/// hashing policy in a module the type knows nothing about — while its
/// first sat in its own `Hash` impl.
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

/// Both lanes in one `write_u64` rather than two `hash_eq` calls on the
/// components — one hasher round per point, on a path that runs per vertex
/// and per shape.
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
