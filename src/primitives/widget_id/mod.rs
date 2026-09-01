//! The identity a widget's cross-frame state hangs off, and the identity
//! hasher a map of them skips hashing with.

use crate::common::hash::Hasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::hash::Hash;
use std::hash::Hasher as _;
use std::panic::Location;

/// Identity hasher for [`WidgetIdMap`]: a [`WidgetId`] is already a
/// hash, so re-hashing it would only cost cycles.
///
/// **Forwarding the id means the map's bucket index is the id's low
/// bits**, so all of this type's distribution comes from whatever
/// produced the id. That is [`WidgetId::finalize`], which every
/// constructor funnels through precisely so this hasher can stay a
/// forward — see there for the measurement and the failure it fixes.
///
/// Before that finalizer existed, the entropy came from `FxHasher`'s
/// own `finish` rotating its high-entropy multiplicative bits down, a
/// rustc-hash 2.x behaviour that 1.x does not have (and 1.x is still
/// reachable in the workspace lock through another crate). That is no
/// longer load-bearing — the finalizer avalanches regardless — but it is
/// worth knowing why this file cares about the version at all.
///
/// The version property itself is still deliberately untested. The
/// obvious test — throw n ids into n buckets and assert the occupancy —
/// measures the wrong thing for *it*: sequential inputs are the best
/// case for a raw multiplicative accumulator, whose low bits are then
/// near a permutation mod 2^k, so 1.x would score *better* than the 2.x
/// behaviour. Anything that genuinely discriminates has to pin
/// rustc-hash's internal mix and would break on a point release that
/// retunes it. `finalize`'s own occupancy test is a different question
/// with a different answer: it pins that *palantir's* ids distribute,
/// which is testable because the finalizer is ours.
#[derive(Debug, Default)]
pub(crate) struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
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

/// The set half of [`WidgetIdMap`], on the same identity hasher. Every
/// per-frame id set is probed once per retained entry, so re-hashing an
/// id that is already a hash is paid per entry per frame.
pub(crate) type WidgetIdSet = HashSet<WidgetId, BuildHasherDefault<IdHasher>>;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WidgetId(pub(crate) u64);

impl WidgetId {
    /// Stable id for the `Layer::Main` synthetic viewport root.
    /// Hard-coded (rather than derived from `auto_stable()` at the
    /// viewport construction site) so refactors to `ui/mod.rs` don't
    /// shift it. Treated like any other parent by
    /// `Ui::widget` — top-level `id_salt("k")` resolves to
    /// `VIEWPORT.with(from_hash("k").0)`.
    pub(crate) const VIEWPORT: Self = Self(u64::MAX);

    pub fn from_hash(h: impl Hash) -> Self {
        let mut hasher = Hasher::new();
        h.hash(&mut hasher);
        Self::finalize(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id. Useful for nested widgets
    /// where the parent already has a stable id — widget authors use this to
    /// key the child nodes they open inside their `show` body.
    pub fn with(self, h: impl Hash) -> Self {
        let mut hasher = Hasher::new();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self::finalize(hasher.finish())
    }

    /// Avalanche a raw [`Hasher`] output into the final id, and keep it
    /// out of the all-zero value, so that a zeroed or
    /// [`Default`]-constructed id can never equal a derived one.
    ///
    /// **Every constructor funnels through here**, which is the point:
    /// the clustering below is a property of `FxHasher`'s output, not of
    /// any one derivation, so fixing it at the shared exit fixes
    /// `from_hash`, `with` and `auto_stable` together.
    ///
    /// # Why an avalanche step at all
    ///
    /// A `WidgetId` is used as its own hash — [`IdHasher`] forwards it
    /// verbatim — so a `WidgetIdMap`'s bucket index *is* the id's low
    /// bits. `FxHasher` mixes by `hash = (hash + word) * K` and its
    /// `finish` rotates left by 26, so those low bits are bits 38..50 of
    /// a value that is *linear* in the last word written. Feed it
    /// sequential inputs — `parent.with(row_index)`, the pattern behind
    /// every list, tree and repeated widget — and consecutive ids differ
    /// by a constant stride in exactly the bits hashbrown buckets on. The
    /// stride shares a factor of two with the table size, so half the
    /// buckets are unreachable:
    ///
    /// ```text
    ///                          4096 ids into 4096 buckets, occupied:
    ///                                       before      after
    ///   WidgetId::from_hash(i)                1998       2608
    ///   parent.with(i)                        1999       2608
    ///   WidgetId::from_hash(i).with("label")  1192       2594
    ///   parent.with(i).with(0)                2143       2591
    ///   uniform expectation ~ n(1 - 1/e)      2589
    /// ```
    ///
    /// The mix is a bijection, so it introduces no collision of its own:
    /// every distinctness argument elsewhere (notably
    /// [`Self::auto_stable`]'s claim that its space cannot alias
    /// [`Self::from_hash`]'s) survives unchanged, because distinct inputs
    /// still map to distinct outputs. That is also what keeps the zero
    /// check exact — `0` is the unique preimage of `0`, so testing the
    /// finalized value is equivalent to testing the raw one, and only one
    /// test is needed.
    ///
    /// The zero displacement itself is the one non-injective step: `1`
    /// ends up with two preimages, the empty hash and whatever the mix
    /// sends there — a 1-in-2^64 collision, which the finalizer neither
    /// adds to nor removes. `1` and not `u64::MAX`, because
    /// [`Self::VIEWPORT`] holds that one.
    ///
    /// This is *not* a fix for the hazard [`IdHasher`]'s doc describes.
    /// That one is about `FxHasher::finish`'s rotate existing at all; a
    /// finalizer here makes the low bits good regardless of whether the
    /// rotate happens, so it happens to defuse that hazard too — but the
    /// doc stays, because the reason it is untestable is unchanged.
    ///
    /// # Cost, and why it is not the cheap variant
    ///
    /// splitmix64's finalizer: five ALU ops on a dependency chain, once
    /// per id construction. Measured against the construction it rides on
    /// (min of 12 interleaved rounds, release, ns per id):
    ///
    /// ```text
    ///                    with()   auto_stable()
    ///   no finalizer      0.892      2.468
    ///   5 ops (this)      1.200      3.205
    ///   4 ops             1.176      3.079
    ///   3 ops             1.116      2.955
    /// ```
    ///
    /// So ~0.3–0.7 ns, against ~51 ns per recorded item on
    /// `record_pass/groups/per_item` — under 1.5 % of the record pass and
    /// far less of a frame. Shaving it to three ops (`x ^= x >> 32;
    /// x *= K; x ^= x >> 32`) buys ~0.25 ns and scores identically on the
    /// occupancy table above, but on that table only: it is one mix pass
    /// rather than two, so it has thinner margin against an input pattern
    /// nobody has measured yet. Not a trade worth making at this price —
    /// the failure mode it would risk is the diffuse, symptomless one
    /// this whole function exists to remove.
    const fn finalize(h: u64) -> Self {
        let mut x = h;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        Self(if x == 0 { 1 } else { x })
    }

    /// Stable across frames as long as the call site is unchanged.
    ///
    /// Hashes the caller's `(file, line, column)` through the crate's
    /// FxHash-backed `Hasher`.
    /// `Location::caller()` resolves at *runtime*, so this runs on every
    /// widget constructor call — with a byte-serial FNV-1a over the file
    /// path it was the single largest record-pass cost in the frame
    /// profile (~90% of `Button::new` self-time); FxHasher walks the
    /// path a word at a time. The [`Self::from_hash`] space can't alias
    /// this one: `str`'s `Hash` impl appends a `0xff` terminator that the
    /// raw byte-slice write here never produces.
    ///
    /// Repeated calls from the same source location (a loop or a closure
    /// helper) all produce the same id; id resolution silently disambiguates by
    /// mixing in a per-id occurrence counter. Override with
    /// [`Configure::id_salt`](crate::scene::node::configure::Configure::id_salt) when call order isn't
    /// stable across frames.
    #[track_caller]
    pub fn auto_stable() -> Self {
        let l = Location::caller();
        let mut hasher = Hasher::new();
        hasher.write(l.file().as_bytes());
        hasher.write_u32(l.line());
        hasher.write_u32(l.column());
        Self::finalize(hasher.finish())
    }
}

#[cfg(test)]
mod tests;
