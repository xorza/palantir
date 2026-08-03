use crate::common::hash::Hasher;
use std::collections::HashMap;
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
    /// out of the all-zero value that would collide with
    /// [`Self::default`] (the "unset" sentinel used by
    /// [`crate::scene::node::Node::new`]).
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
    /// sends there. A 1-in-2^64 collision, unchanged from before this
    /// finalizer existed. `1` and not `u64::MAX`, because
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
    /// [`crate::scene::node::Configure::id_salt`] when call order isn't
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
mod tests {
    use crate::primitives::widget_id::WidgetId;
    use rustc_hash::FxHasher;
    use std::hash::Hasher;

    /// Both calls resolve to the *same* caller location, letting the test
    /// below rebuild the exact hash `auto_stable` must produce.
    #[track_caller]
    fn id_and_loc() -> (WidgetId, &'static std::panic::Location<'static>) {
        (WidgetId::auto_stable(), std::panic::Location::caller())
    }

    #[test]
    fn auto_stable_hashes_location_via_fx() {
        let (id, l) = id_and_loc();
        // Deliberately the *raw* `FxHasher`, not the crate wrapper the
        // production path now uses: rebuilding the expected value with
        // the same type under test would assert nothing. This is the
        // cross-check that the wrapper still hashes like plain FxHash.
        let mut hasher = FxHasher::default();
        hasher.write(l.file().as_bytes());
        hasher.write_u32(l.line());
        hasher.write_u32(l.column());
        // `finalize` is applied through the function under test rather
        // than re-spelled, so this stays a cross-check of the *hashing*
        // half. `finalize_avalanches_sequential_ids` covers the other.
        assert_eq!(id, WidgetId::finalize(hasher.finish()));

        // Same call site (loop) → identical ids; a different call line →
        // a different id.
        let repeated: Vec<WidgetId> = (0..2).map(|_| id_and_loc().0).collect();
        assert_eq!(repeated[0], repeated[1]);
        assert_ne!(repeated[0], id);
    }

    /// The property [`WidgetId::finalize`] exists for: ids derived from
    /// *sequential* inputs — every list row, tree node and repeated
    /// widget — must land across the low bits, because those bits are
    /// the bucket index of every `WidgetIdMap` in the crate.
    ///
    /// Asserted statistically rather than against fixed values, so it
    /// survives a deliberate change of mix while still failing on one
    /// that reintroduces the stride. Throwing `n` ids into `n` buckets,
    /// the count of *distinct* buckets is the classic occupancy result,
    /// `n(1 - 1/e)` ≈ 2589 of 4096. The pre-finalizer values ranged
    /// 1192–2143, so a floor of 2400 sits clear of every one of them and
    /// well under the ~2590 a good mix produces.
    ///
    /// All four derivation shapes are covered because they failed by
    /// different amounts, and the worst was not the obvious one:
    /// `from_hash(i).with("label")` — a per-row widget naming an
    /// internal part — was the 1192.
    #[test]
    fn finalize_avalanches_sequential_ids() {
        const N: usize = 4096;
        const MASK: u64 = N as u64 - 1;
        // Uniform expectation is ~2589; the un-finalized mixes scored
        // 1192–2143.
        const FLOOR: usize = 2400;

        let parent = WidgetId::from_hash("row-parent");
        let cases: [(&str, Vec<WidgetId>); 4] = [
            ("from_hash(i)", (0..N).map(WidgetId::from_hash).collect()),
            ("parent.with(i)", (0..N).map(|i| parent.with(i)).collect()),
            (
                "from_hash(i).with(part)",
                (0..N)
                    .map(|i| WidgetId::from_hash(i).with("label"))
                    .collect(),
            ),
            (
                "parent.with(i).with(part)",
                (0..N).map(|i| parent.with(i).with(0)).collect(),
            ),
        ];
        for (label, ids) in cases {
            let buckets: std::collections::HashSet<u64> =
                ids.iter().map(|id| id.0 & MASK).collect();
            assert!(
                buckets.len() >= FLOOR,
                "{label}: {N} ids landed in {} of {N} low-bit buckets, under \
                 the {FLOOR} floor — sequential ids have regained a constant \
                 stride in the bits hashbrown buckets on, and every \
                 WidgetIdMap is now clustering",
                buckets.len(),
            );
        }
    }

    /// `finalize` must be a bijection: it is applied to an already-unique
    /// hash, so anything that folded two inputs together would manufacture
    /// `WidgetId` collisions out of nothing — and a collision here is two
    /// widgets silently sharing state, focus and layout rows.
    ///
    /// Checked two ways, because neither alone is convincing. Exhaustive
    /// injectivity is impossible over `u64`, so: a large sweep of the
    /// sequential inputs the ids actually come from must produce no
    /// duplicate, and the mix must be invertible by construction — every
    /// step is an xor-shift or an odd multiply, so the inverse exists and
    /// round-trips.
    #[test]
    fn finalize_is_a_bijection_that_avoids_zero() {
        // Invert splitmix64's finalizer: odd multiplies invert via their
        // modular inverse, `x ^= x >> s` by re-folding the shift until it
        // converges (`64 / s + 1` rounds suffice, since each round
        // recovers another `s` bits).
        fn unxorshift(y: u64, shift: u32) -> u64 {
            let mut x = y;
            for _ in 0..64 / shift + 1 {
                x = y ^ (x >> shift);
            }
            x
        }
        const INV_A: u64 = 0x96de_1b17_3f11_9089; // inverse of 0xbf58476d1ce4e5b9
        const INV_B: u64 = 0x3196_42b2_d24d_8ec3; // inverse of 0x94d049bb133111eb
        assert_eq!(0xbf58_476d_1ce4_e5b9u64.wrapping_mul(INV_A), 1);
        assert_eq!(0x94d0_49bb_1331_11ebu64.wrapping_mul(INV_B), 1);

        // Deduplicated first: the three shapes overlap (at `i == 0` all
        // three are zero), and a fixture feeding one input twice would
        // report its own duplicate as a collision.
        let raws: std::collections::HashSet<u64> = (0..200_000u64)
            .flat_map(|i| [i, i << 32, i.wrapping_mul(0x9e37_79b9_7f4a_7c15)])
            .collect();
        let mut images = std::collections::HashSet::with_capacity(raws.len());
        for &raw in &raws {
            let id = WidgetId::finalize(raw);
            assert_ne!(id.0, 0, "finalize must never produce the unset sentinel");
            if raw != 0 {
                // Round-trip proves this input was not folded onto
                // another. `raw == 0` is the one value the zero-guard
                // deliberately displaces, so it has no preimage to
                // recover.
                let mut x = unxorshift(id.0, 31);
                x = x.wrapping_mul(INV_B);
                x = unxorshift(x, 27);
                x = x.wrapping_mul(INV_A);
                assert_eq!(unxorshift(x, 30), raw, "finalize is not invertible");
            }
            images.insert(id.0);
        }
        assert_eq!(
            images.len(),
            raws.len(),
            "finalize folded two distinct ids together",
        );
        // Zero is displaced to 1 rather than left as the unset sentinel,
        // and `1` specifically because `u64::MAX` is taken by
        // [`WidgetId::VIEWPORT`].
        //
        // This displacement is the *only* place injectivity is given up:
        // `1` now has two preimages, `0` and whatever the mix maps to
        // `1`. That is a 1-in-2^64 collision between one specific hash
        // and the empty one — the same class as any other hash collision
        // the crate already accepts, and identical to what the
        // pre-finalizer code did.
        assert_eq!(WidgetId::finalize(0), WidgetId(1));
    }
}
