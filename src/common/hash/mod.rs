//! Shared hashing primitive: `FxHasher` wrapped to add a whole-value `pod()`
//! write beside the `Hasher` trait; use it instead of `FxHasher::default()`.
//! FxHasher beat foldhash and ahash in the per-frame micro-shootout (~2.4-3.3x
//! on the node+shape mix) because our writes are many and small.

use rustc_hash::FxHasher;
use std::fmt;
use std::hash;
use std::hash::Hasher as _;

/// Canonical FxHash of a `str`'s bytes, the content hash stored by `RecordedText`.
pub(crate) fn hash_str(s: &str) -> u64 {
    use std::hash::Hash;
    let mut h = Hasher::new();
    s.hash(&mut h);
    h.finish()
}

/// `FxHasher` plus an inherent `pod()`; implements `Hasher`.
#[derive(Clone)]
pub(crate) struct Hasher(FxHasher);

// Manual: `FxHasher` has no `Debug`.
impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::hash::Hasher as _;
        f.debug_tuple("Hasher").field(&self.0.finish()).finish()
    }
}

impl Hasher {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(FxHasher::default())
    }

    /// Hashes a slice of pod values as one contiguous byte run (`NoUninit`
    /// proves no padding). For a column written long before, not a value just
    /// built, whose field-by-field stores the CPU cannot forward.
    ///
    /// Does **not** write the length: fold it in for variable-length columns.
    /// **Not hash-equal to a per-element [`Self::pod`] loop**, so fine for a hash
    /// compared only against itself, wrong for anything persisted. Pinned by
    /// `pod_slice_differs_from_element_wise_pod`.
    #[inline]
    pub(crate) fn pod_slice<T: bytemuck::NoUninit>(&mut self, v: &[T]) {
        self.0.write(bytemuck::cast_slice(v));
    }
}

impl hash::Hasher for Hasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0.finish()
    }

    // Forward integer writes to `FxHasher`: the default impls detour through `write(&[u8])`.
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.0.write_u8(i);
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.0.write_u16(i);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0.write_u32(i);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0.write_u64(i);
    }
    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.0.write_u128(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.0.write_usize(i);
    }
}

#[cfg(test)]
mod tests;
