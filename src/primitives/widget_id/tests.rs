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
        let buckets: std::collections::HashSet<u64> = ids.iter().map(|id| id.0 & MASK).collect();
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
