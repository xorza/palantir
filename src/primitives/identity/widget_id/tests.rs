use crate::primitives::identity::widget_id::WidgetId;
use rustc_hash::FxHasher;
use std::collections;
use std::hash::Hasher;
use std::panic;

#[track_caller]
fn id_and_loc() -> (WidgetId, &'static panic::Location<'static>) {
    (WidgetId::auto(), panic::Location::caller())
}

#[test]
fn auto_hashes_location_via_fx() {
    let (id, l) = id_and_loc();
    // The raw `FxHasher`, not the crate wrapper: rebuilding with the type under test asserts nothing.
    let mut hasher = FxHasher::default();
    hasher.write(l.file().as_bytes());
    hasher.write_u32(l.line());
    hasher.write_u32(l.column());
    // `finalize` goes through the function under test, so this cross-checks only the hashing half.
    assert_eq!(id, WidgetId::finalize(hasher.finish()));
    assert_eq!(WidgetId::from_location(l), id);

    let repeated: Vec<WidgetId> = (0..2).map(|_| id_and_loc().0).collect();
    assert_eq!(repeated[0], repeated[1]);
    assert_ne!(repeated[0], id);
}

/// Ids from sequential inputs must spread across the low bits, each
/// `WidgetIdMap`'s bucket index. Asserted statistically: `n` ids into `n`
/// buckets occupy about `n(1 - 1/e)` ≈ 2589 of 4096, while the pre-finalizer
/// mixes scored 1192-2143, so 2400 separates them. All four derivation shapes
/// are covered; `from_hash(i).with("label")` was the worst.
#[test]
fn finalize_avalanches_sequential_ids() {
    const N: usize = 4096;
    const MASK: u64 = N as u64 - 1;
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
        let buckets: collections::HashSet<u64> = ids.iter().map(|id| id.0 & MASK).collect();
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

/// `finalize` must be a bijection, or two inputs fold into one `WidgetId`.
/// Checked by a large sweep with no duplicate, and by inverting the mix
/// (xor-shifts and odd multiplies) to round-trip.
#[test]
fn finalize_is_a_bijection_that_avoids_zero() {
    // Inverts splitmix64's finalizer: odd multiplies by modular inverse, `x ^= x >> s` by re-folding.
    fn unxorshift(y: u64, shift: u32) -> u64 {
        let mut x = y;
        for _ in 0..=(64 / shift) {
            x = y ^ (x >> shift);
        }
        x
    }
    const INV_A: u64 = 0x96de_1b17_3f11_9089; // inverse of 0xbf58476d1ce4e5b9
    const INV_B: u64 = 0x3196_42b2_d24d_8ec3; // inverse of 0x94d049bb133111eb
    assert_eq!(0xbf58_476d_1ce4_e5b9u64.wrapping_mul(INV_A), 1);
    assert_eq!(0x94d0_49bb_1331_11ebu64.wrapping_mul(INV_B), 1);

    let raws: collections::HashSet<u64> = (0..200_000u64)
        .flat_map(|i| [i, i << 32, i.wrapping_mul(0x9e37_79b9_7f4a_7c15)])
        .collect();
    let mut images = collections::HashSet::with_capacity(raws.len());
    for &raw in &raws {
        let id = WidgetId::finalize(raw);
        assert_ne!(id.0, 0, "finalize must never produce the zero value");
        if raw != 0 {
            // The round-trip proves no folding; `raw == 0` is displaced by the zero-guard and has no preimage.
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
    // Zero is displaced to 1 since `u64::MAX` is `WidgetId::VIEWPORT`: the only
    // place injectivity is given up, a 1-in-2^64 collision.
    assert_eq!(WidgetId::finalize(0), WidgetId(1));
}
