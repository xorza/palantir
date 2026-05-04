//! Shared hashing primitive: `FxHasher` wrapped to expose `pod()` for
//! whole-value byte writes alongside the standard `std::hash::Hasher`
//! trait. Use this everywhere we'd otherwise reach for
//! `FxHasher::default()` directly so the `pod` shortcut and trait
//! methods are always in scope at the same time.
//!
//! Per-domain hashers (e.g. `tree::hash::compute_node_hash`) build on
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
