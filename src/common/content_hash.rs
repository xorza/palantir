//! The authoring fingerprint every cross-frame cache keys on: whether what a node declared changed.

use std::hash::Hasher;
use std::ops::BitXor;

/// Authoring fingerprint shared by tree rollups, shape/chrome records,
/// layout caches, text shaping, cascade, and damage.
///
/// [`Hash`] feeds the fingerprint as one `write_u64`, so a parent's hash folds in a child's. [`Ord`]
/// means nothing of the content: it is the order a sort key needs.
#[repr(transparent)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::Pod,
    bytemuck::Zeroable,
)]
pub(crate) struct ContentHash(u64);

impl ContentHash {
    #[inline]
    pub(crate) const fn from_u64(bits: u64) -> Self {
        Self(bits)
    }

    /// What `hasher` holds.
    #[inline]
    pub(crate) fn finish(hasher: &impl Hasher) -> Self {
        Self(hasher.finish())
    }

    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }
}

/// The two fingerprints as one, for a row whose pixels follow a second input. The default is its
/// identity, so an absent input leaves a hash as it is.
impl BitXor for ContentHash {
    type Output = Self;

    #[inline]
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::hash::Hasher as WorkspaceHasher;
    use std::hash::Hash;

    /// `from_u64` and `as_u64` carry the bits. `0b1100 ^ 0b1010 = 0b0110`, and 0 changes nothing.
    /// `finish` reads the hasher and `Hash` feeds it as `write_u64` does.
    #[test]
    fn content_hash_carries_its_bits() {
        let hash = ContentHash::from_u64(0b1100);
        assert_eq!(hash.as_u64(), 0b1100);
        assert_eq!((hash ^ ContentHash::from_u64(0b1010)).as_u64(), 0b0110);
        assert_eq!(hash ^ ContentHash::default(), hash);

        let mut fed = WorkspaceHasher::new();
        hash.hash(&mut fed);
        let mut written = WorkspaceHasher::new();
        written.write_u64(0b1100);
        assert_eq!(ContentHash::finish(&fed).as_u64(), written.finish());
    }
}
