use crate::common::hash::Hasher;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::color::ColorU8;
use std::hash::{Hash, Hasher as _};

/// Two-stop gradient, the shape almost every UI gradient takes.
fn ramp(a: ColorU8, b: ColorU8) -> GradientStops {
    GradientStops::new([Stop::new(0.0, a), Stop::new(1.0, b)])
}

/// Entries landing on the most crowded bucket index, hashing `keys`
/// the way `FxHashMap` would: `hash & (buckets - 1)`.
fn worst_bucket(keys: &[GradientStops], buckets: usize) -> usize {
    let mut counts = vec![0usize; buckets];
    for key in keys {
        let mut h = Hasher::new();
        key.hash(&mut h);
        counts[(h.finish() as usize) & (buckets - 1)] += 1;
    }
    counts.into_iter().max().unwrap_or(0)
}

/// Structured palettes must spread across buckets, not pile onto one.
///
/// This is the property the byte layout exists for, and nothing else
/// observes it: a clustering layout returns identical *values* from
/// every lookup and fails only as a silent 10-30x slowdown in
/// `CpuGradientAtlas`'s index. The three populations are ordinary
/// ways to generate a palette, and the previous layout — colour in
/// the high half — put all 200 of the first two into a **single**
/// bucket.
///
/// The bound is deliberately loose. 200 keys in 256 buckets average
/// 0.8 each and a sound hash peaks around 4-5; the failure being
/// guarded against peaks at 200. Anything under 16 means the low bits
/// carry real entropy, which is the whole claim.
#[test]
fn hash_spreads_across_buckets_for_structured_palettes() {
    const N: u32 = 200;
    const BUCKETS: usize = 256;
    const LIMIT: usize = 16;

    // One hue family: blue pinned, red/green varying — themed accents
    // generated off a base colour.
    let same_blue: Vec<GradientStops> = (0..N)
        .map(|i| {
            ramp(
                ColorU8::rgb(i as u8, (i * 3) as u8, 0x2e),
                ColorU8::rgb(0x4c, (i * 5) as u8, 0x2e),
            )
        })
        .collect();
    // A greyscale/monochrome ramp: all channels move together.
    let mono: Vec<GradientStops> = (0..N)
        .map(|i| {
            let v = (i as u8).wrapping_mul(2);
            ramp(ColorU8::rgb(v, v, v), ColorU8::rgb(v / 2, v / 2, v / 2))
        })
        .collect();
    // Only the red channel moves — the degenerate end of the range.
    let red_only: Vec<GradientStops> = (0..N)
        .map(|i| ramp(ColorU8::rgb(i as u8, 0, 0), ColorU8::rgb(0x40, 0, 0)))
        .collect();

    for (label, keys) in [
        ("same-blue accent family", &same_blue),
        ("monochrome ramp", &mono),
        ("red-channel-only", &red_only),
    ] {
        let worst = worst_bucket(keys, BUCKETS);
        assert!(
            worst <= LIMIT,
            "{label}: {worst} of {N} gradients share one bucket index \
                 (limit {LIMIT}) — the stop hash is clustering, which \
                 degrades every gradient-atlas lookup",
        );
    }
}

/// Distinct content still hashes distinctly — the spread above must
/// not have come from collapsing information. Offset and colour
/// occupy disjoint halves of one word, so a colour can never alias
/// an offset.
#[test]
fn offset_and_colour_stay_independent() {
    let base = ramp(ColorU8::rgb(1, 2, 3), ColorU8::rgb(4, 5, 6));
    let colour_swapped = ramp(ColorU8::rgb(4, 5, 6), ColorU8::rgb(1, 2, 3));
    let offset_moved = GradientStops::new([
        Stop::new(0.0, ColorU8::rgb(1, 2, 3)),
        Stop::new(0.5, ColorU8::rgb(4, 5, 6)),
    ]);

    let digest = |s: &GradientStops| {
        let mut h = Hasher::new();
        s.hash(&mut h);
        h.finish()
    };
    assert_ne!(digest(&base), digest(&colour_swapped));
    assert_ne!(digest(&base), digest(&offset_moved));
    assert_eq!(digest(&base), digest(&base.clone()));
}

/// Written order must not reach cache identity: the stops *are* the
/// gradient's key, so two spellings of one ramp that bake the same
/// LUT row have to hash and compare the same, or they take two atlas
/// rows and two eviction slots apiece.
#[test]
fn written_order_does_not_reach_identity() {
    let a = ColorU8::rgb(1, 2, 3);
    let b = ColorU8::rgb(4, 5, 6);
    let c = ColorU8::rgb(7, 8, 9);
    let ascending = GradientStops::new([Stop::new(0.0, a), Stop::new(0.5, b), Stop::new(1.0, c)]);
    let shuffled = GradientStops::new([Stop::new(1.0, c), Stop::new(0.0, a), Stop::new(0.5, b)]);

    let digest = |s: &GradientStops| {
        let mut h = Hasher::new();
        s.hash(&mut h);
        h.finish()
    };
    assert_eq!(ascending, shuffled, "reordered input is the same gradient");
    assert_eq!(digest(&ascending), digest(&shuffled));
    // Sorted on the way in, so the stored sequence is ascending
    // whichever order it was written in.
    let offsets: Vec<u8> = shuffled.iter().map(|s| s.offset_u8).collect();
    assert_eq!(offsets, vec![0, 128, 255]);

    // Equal offsets keep their written order, so a hard break still
    // reads in the direction it was authored.
    let break_ab = GradientStops::new([Stop::new(0.5, a), Stop::new(0.5, b)]);
    let break_ba = GradientStops::new([Stop::new(0.5, b), Stop::new(0.5, a)]);
    assert_ne!(break_ab, break_ba);
}
