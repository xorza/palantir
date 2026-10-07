//! The identity a widget's cross-frame state hangs off, and the hasher a map of them skips hashing with.

use crate::common::hash::Hasher;
use std::collections::{HashMap, HashSet};
use std::hash;
use std::hash::BuildHasherDefault;
use std::hash::Hash;
use std::hash::Hasher as _;
use std::panic::Location;

/// Identity hasher for [`WidgetIdMap`]: a [`WidgetId`] is already a hash, so re-hashing is wasted.
///
/// Forwarding means the bucket index is the id's low bits, so distribution comes entirely from [`WidgetId::finalize`], which every constructor funnels through.
///
/// The `rustc-hash` version behaviour that once supplied entropy is deliberately untested: occupancy on sequential inputs would score 1.x better, and a discriminating test pins rustc-hash internals.
#[derive(Debug, Default)]
pub(crate) struct IdHasher(u64);

impl hash::Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }

    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("IdHasher only sees write_u64 from WidgetId's derived Hash impl");
    }
}

pub(crate) type WidgetIdMap<V> = HashMap<WidgetId, V, BuildHasherDefault<IdHasher>>;

/// The set half of [`WidgetIdMap`], on the same identity hasher; per-frame id sets are probed once per retained entry.
pub(crate) type WidgetIdSet = HashSet<WidgetId, BuildHasherDefault<IdHasher>>;

/// A widget's identity across frames, derived from the call site and any [`Configure::id_salt`](crate::Configure::id_salt) above it, so the same call in the same place gives the same id. Cross-frame state, focus and animation key on it.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WidgetId(pub(crate) u64);

impl WidgetId {
    /// Id of the `Layer::Main` synthetic viewport root. Hard-coded so refactors of `ui/mod.rs` don't shift it; a top-level `id_salt("k")` resolves to `VIEWPORT.with(from_hash("k").0)`.
    pub(crate) const VIEWPORT: Self = Self(u64::MAX);

    /// An id from anything hashable, with no parent mixed in; [`Self::with`] derives a child.
    pub fn from_hash(h: impl Hash) -> Self {
        let mut hasher = Hasher::new();
        h.hash(&mut hasher);
        Self::finalize(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id; used to key child nodes opened inside a `show` body.
    #[must_use]
    pub fn with(self, h: impl Hash) -> Self {
        let mut hasher = Hasher::new();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self::finalize(hasher.finish())
    }

    /// Avalanche a raw [`Hasher`] output into the final id, and keep it off all-zero so a zeroed or [`Default`] id never equals a derived one. Every constructor funnels through here.
    ///
    /// A `WidgetId` is its own hash ([`IdHasher`] forwards it), so a map's bucket index is the id's low bits. `FxHasher` leaves those linear in the last word written, so sequential inputs (`parent.with(row_index)`) stride by a constant sharing a factor of two with the table size and half the buckets go unreachable (4096 sequential ids occupied ~2000 buckets, against ~2590 for uniform).
    ///
    /// The mix (splitmix64's finalizer) is a bijection, so distinctness arguments elsewhere survive and the zero check is exact. The one non-injective step is the displacement of zero to `1` (a 1-in-2^64 collision); `1` not `u64::MAX`, because [`Self::VIEWPORT`] holds that.
    ///
    /// The cost is five ALU ops per id, well under 1% of the record pass; a three-op variant has thinner margin against untested input patterns.
    const fn finalize(h: u64) -> Self {
        let mut x = h;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        Self(if x == 0 { 1 } else { x })
    }

    /// Stable across frames while the call site is unchanged. Repeated calls from one location (a loop or closure helper) get an occurrence counter mixed in; use [`Configure::id_salt`](crate::widget_core::configure::Configure::id_salt) when call order is unstable.
    #[track_caller]
    pub fn auto() -> Self {
        Self::from_location(Location::caller())
    }

    /// `self` under `parent`, or `self` where nothing is open (a side layer's root).
    #[inline]
    #[must_use]
    pub(crate) fn scoped(self, parent: Option<Self>) -> Self {
        match parent {
            Some(p) => p.with(self.0),
            None => self,
        }
    }

    /// The id [`Self::auto`] gives at `site`: its `(file, line, column)` through the crate's FxHash `Hasher`. It cannot alias [`Self::from_hash`]: `str`'s `Hash` appends a `0xff` terminator the raw byte write here never produces.
    ///
    /// Hashing the file path costs a word at a time, so widgets keep the `Location` and hash it only when the id tracker cannot match the call site to last frame's (see `SeenIds::resolve_scoped`).
    pub(crate) fn from_location(site: &Location<'_>) -> Self {
        let mut hasher = Hasher::new();
        hasher.write(site.file().as_bytes());
        hasher.write_u32(site.line());
        hasher.write_u32(site.column());
        Self::finalize(hasher.finish())
    }
}

#[cfg(test)]
mod tests;
