//! Shared hashing primitive: `FxHasher` wrapped to expose `pod()` for
//! whole-value byte writes alongside the standard `std::hash::Hasher`
//! trait. Use this everywhere we'd otherwise reach for
//! `FxHasher::default()` directly so the `pod` shortcut and trait
//! methods are always in scope at the same time.
//!
//! Per-domain hashers (e.g. `Tree::compute_rollups`) build on
//! top of this — they own the field-walk and tagged-union policy;
//! this module owns just the streaming primitive.
//!
//! FxHasher won the per-frame hashing micro-shootout against foldhash
//! (which sponges into a u128 — slower than FxHasher's
//! rotate-mul-xor per `write_u8` / `write_u32` for our many-small-writes
//! pattern). Re-checked after migrating several hot paths to bulk
//! `write(&[u8])` pod writes (Vec2/Size/Rect/GridCell): FxHasher still
//! wins ~2.4-3.3x on the realistic node+shape mix and ~2x on `write_u64`
//! subtree rollups, vs foldhash and ahash. foldhash only edges ahead
//! (~10%) on contiguous buffers ≥16 B — and our pod writes are almost
//! all ≤8 B, so the migration never tipped the balance.

use rustc_hash::FxHasher;
use std::hash::Hasher as _;

/// Canonical FxHash of a `str`'s bytes — the content hash stored by
/// `RecordedText`. Computed while `RecordStore::record_text` lowers a
/// handle so the hash always describes the bytes addressed by its final span.
pub(crate) fn hash_str(s: &str) -> u64 {
    use std::hash::Hash;
    let mut h = Hasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Wrapper around `FxHasher` that adds an inherent `pod()` method.
/// Implements `std::hash::Hasher` so `value.hash(&mut h)` and
/// `h.write_u8(...)` etc. work unchanged when the trait is in scope
/// (`use std::hash::Hasher as _;`).
#[derive(Clone)]
pub(crate) struct Hasher(FxHasher);

// Manual: `FxHasher` has no `Debug`, and its state is one opaque `u64`.
// The digest so far is the only thing worth showing, and reading it is
// non-destructive.
impl std::fmt::Debug for Hasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::hash::Hasher as _;
        f.debug_tuple("Hasher").field(&self.0.finish()).finish()
    }
}

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

    /// Hash a whole slice of pod values as one contiguous byte run —
    /// the [`Self::pod`] counterpart for columns. Same `NoUninit`
    /// bound, same reason: no padding means the byte view is
    /// well-defined, so the cast is sound.
    ///
    /// This is where a bulk write actually pays. `FxHasher::write`
    /// consumes 8 bytes per iteration, so hashing an N-element column
    /// in one call costs roughly `size_of::<T>() * N / 8` mix ops
    /// instead of one per field per element.
    ///
    /// Does **not** write the length. A caller hashing a variable-length
    /// column alongside other data must fold the length in itself, or
    /// two different splits of the same bytes collide.
    ///
    /// **Not hash-equal to a per-element [`Self::pod`] loop.**
    /// `FxHasher::write` consumes its input in `usize`-sized chunks, so one
    /// 16-byte write and two 8-byte writes end in different states.
    /// Switching a loop to this method changes the value it produces — fine
    /// for a hash only ever compared against itself (a cache key recomputed
    /// each run), wrong for anything persisted or compared across a version
    /// boundary. Pinned by `pod_slice_differs_from_element_wise_pod`.
    #[inline]
    pub(crate) fn pod_slice<T: bytemuck::NoUninit>(&mut self, v: &[T]) {
        self.0.write(bytemuck::cast_slice(v));
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

    // Forward the integer-width methods directly to `FxHasher`. The
    // default trait impls build an `[u8; N]` slice and route through
    // `write(&[u8])` → `FxHasher::write` → `hash_bytes` (the bulk
    // chunked path), whereas `FxHasher::write_uN` is a single
    // `add_to_hash` mix op. Most of `Tree::compute_rollups` and
    // `Shapes::add`'s per-node/per-shape work is tiny `write_u8`s and
    // `write_u64`s; skipping the slice detour folds many cycles.
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
