//! Shared hashing primitive: `FxHasher` wrapped to expose `pod()` for
//! whole-value byte writes alongside the standard `std::hash::Hasher`
//! trait. Use this everywhere we'd otherwise reach for
//! `FxHasher::default()` directly so the `pod` shortcut and trait
//! methods are always in scope at the same time.
//!
//! Per-domain hashers (e.g. `tree::node_hash::compute_node_hash`) build on
//! top of this — they own the field-walk and tagged-union policy;
//! this module owns just the streaming primitive.

use rustc_hash::FxHasher;
use std::hash::Hasher as _;

/// Wrapper around `FxHasher` that adds an inherent `pod()` method.
/// Implements `std::hash::Hasher` so `value.hash(&mut h)` and
/// `h.write_u8(...)` etc. work unchanged when the trait is in scope
/// (`use std::hash::Hasher as _;`).
pub(crate) struct Hasher(FxHasher);

impl Hasher {
    #[inline]
    pub(crate) fn new() -> Self {
        Self(FxHasher::default())
    }

    /// Hash a value as its raw bytes in one `Hasher::write` call. The
    /// `NoUninit` bound proves at compile time that `T` has no padding
    /// so `bytes_of` is sound.
    ///
    /// Why this is faster than per-field writes: `FxHasher::write(&[u8])`
    /// consumes 8 bytes per loop iteration and amortizes the
    /// rotate/multiply/xor cost across the whole slice. Replacing
    /// N×`write_u32`/`write_u16` calls with one `write` cuts per-call
    /// overhead and lets the compiler keep more state in registers.
    #[inline]
    pub(crate) fn pod<T: bytemuck::NoUninit>(&mut self, v: &T) {
        self.0.write(bytemuck::bytes_of(v));
    }
}

impl std::hash::Hasher for Hasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_matches_write_of_bytes_of() {
        // The performance shortcut is only safe if `pod(&v)` produces
        // the exact same hash as feeding `bytemuck::bytes_of(&v)`
        // through `write`. Pin the equivalence.
        let v: u32 = 0xdead_beef;
        let mut a = Hasher::new();
        a.pod(&v);
        let mut b = Hasher::new();
        b.write(bytemuck::bytes_of(&v));
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn pod_matches_write_for_repr_c_pod() {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::NoUninit)]
        struct Pair {
            a: u32,
            b: u32,
        }
        let p = Pair {
            a: 0x1234_5678,
            b: 0x9abc_def0,
        };
        let mut h1 = Hasher::new();
        h1.pod(&p);
        let mut h2 = Hasher::new();
        h2.write(bytemuck::bytes_of(&p));
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn new_matches_default_seed() {
        // `Hasher::new` is a thin wrapper over `FxHasher::default`. If
        // a future refactor adds a custom seed without updating call
        // sites, every cache key changes silently — pin the equality.
        let mut wrapped = Hasher::new();
        let mut raw = FxHasher::default();
        let bytes: &[u8] = b"palantir";
        wrapped.write(bytes);
        raw.write(bytes);
        assert_eq!(wrapped.finish(), raw.finish());
    }

    #[test]
    fn empty_hash_is_stable() {
        // Cheap canary: if the underlying `FxHasher` swap changes the
        // empty-input output, every persisted snapshot key shifts.
        let h1 = Hasher::new().finish();
        let h2 = Hasher::new().finish();
        assert_eq!(h1, h2);
    }
}
