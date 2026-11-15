//! [`ContentHash`]: the 64-bit hash every cross-frame cache compares to see whether content changed.

use crate::common::hash::Hasher;
use std::hash::{self, Hash};

/// A 64-bit hash of content, which tree rollups, shape and chrome records, layout caches, text
/// shaping, cascade and damage compare across frames, and which
/// [`Mesh::content_hash`](crate::Mesh::content_hash) returns.
///
/// Equal content gives an equal hash within one process. The value is not for storage: a text
/// hashes under a seed drawn once per process, and a run of values hashes as one byte write.
///
/// [`Hash`] feeds the hash as one `write_u64`, so a parent's hash folds in a child's. [`Ord`] means
/// nothing of the content: it is the order a sort key needs.
///
/// A caller compares a hash and never makes one, so the only public constructor is [`Default`]:
/// structs that hold a hash derive it, and its zero is the identity of the crate's XOR. `NoUninit`
/// stands in for `Pod`, which would let a caller cast chosen bits into a hash. Refused: the name
/// `Hash64`, as rustc has it, which gives only the width.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::NoUninit)]
pub struct ContentHash(u64);

impl ContentHash {
    /// The hash of `value`: equal values give equal hashes.
    #[inline]
    pub(crate) fn of(value: impl Hash) -> Self {
        let mut hasher = Hasher::new();
        value.hash(&mut hasher);
        Self::finish(&hasher)
    }

    /// What `hasher` holds.
    #[inline]
    pub(crate) fn finish(hasher: &impl hash::Hasher) -> Self {
        Self(hasher.finish())
    }

    #[inline]
    pub(crate) const fn from_u64(bits: u64) -> Self {
        Self(bits)
    }

    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }

    /// The two hashes as one, for a row whose pixels follow a second input. The default is its
    /// identity, so an absent input leaves a hash as it is. Refused: `BitXor`, a public impl on
    /// this public type.
    #[inline]
    pub(crate) const fn xor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::common::content_hash::ContentHash;
    use crate::common::hash::Hasher;
    use std::hash::{Hash, Hasher as _};

    /// `from_u64` and `as_u64` carry the bits. `0b1100 ^ 0b1010 = 0b0110`, and 0 changes nothing.
    /// `finish` reads the hasher and `Hash` feeds it as `write_u64` does. `of` hashes the value as
    /// a hasher fed the same word does, and another value gives another hash.
    #[test]
    fn content_hash_carries_its_bits() {
        let hash = ContentHash::from_u64(0b1100);
        assert_eq!(hash.as_u64(), 0b1100);
        assert_eq!(hash.xor(ContentHash::from_u64(0b1010)).as_u64(), 0b0110);
        assert_eq!(hash.xor(ContentHash::default()), hash);

        let mut fed = Hasher::new();
        hash.hash(&mut fed);
        let mut written = Hasher::new();
        written.write_u64(0b1100);
        assert_eq!(ContentHash::finish(&fed).as_u64(), written.finish());

        let mut seven = Hasher::new();
        seven.write_u64(7);
        assert_eq!(ContentHash::of(7_u64), ContentHash::finish(&seven));
        assert_ne!(ContentHash::of(7_u64), ContentHash::of(8_u64));
    }
}
