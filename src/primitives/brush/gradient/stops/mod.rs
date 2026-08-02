mod serde;

use crate::primitives::color::ColorU8;
use tinyvec::ArrayVec;

/// Hard cap on stops in a single gradient. 8 covers >99% of UI use
/// (2-3 stops dominate, multi-stop bars rarely exceed 5).
pub(crate) const MAX_STOPS: usize = 8;

/// One colour stop in a gradient. `offset_u8` is the 0..1 parametric
/// position quantized to 8 bits (256 levels — finer than the LUT it
/// bakes into). `color` is 8-bit linear RGB. Total 5 B / stop, align 1, so
/// `GradientStops` is 40 B inline vs. 64 B with f32 offsets.
/// Stops are storage-only (never animated; snap on morph), feed a u8
/// LUT, and out-of-range positions clamp at construction — 8-bit
/// precision is sufficient and saves ~24 B per gradient.
///
/// Serde uses the **float** `offset` (0..1) as the wire form, not the
/// internal `offset_u8` byte — theme authors write `offset = 0.5`,
/// matching how every other spatial value in the crate is authored;
/// the u8 quantization stays an implementation detail.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stop {
    pub offset_u8: u8,
    pub color: ColorU8,
}

impl Stop {
    /// Construct a stop. Finite offsets are clamped to 0..=1 and
    /// quantized to u8 (round-to-nearest).
    #[inline]
    pub fn new(offset: f32, color: impl Into<ColorU8>) -> Self {
        assert!(offset.is_finite(), "gradient stop offset must be finite");
        let q = (offset.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Self {
            offset_u8: q,
            color: color.into(),
        }
    }

    /// Decode the stored quantized position back to a 0..1 f32 for
    /// consumers (atlas bake, axis calc) that interpolate in float.
    #[inline]
    pub const fn offset(self) -> f32 {
        self.offset_u8 as f32 / 255.0
    }
}

/// Inline gradient-stop sequence whose length is always two through eight.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientStops(ArrayVec<[Stop; MAX_STOPS]>);

impl GradientStops {
    /// Collect stops into inline storage, panicking on an invalid count.
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        let mut values = ArrayVec::new();
        for stop in stops {
            assert!(
                values.len() < MAX_STOPS,
                "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}",
            );
            values.push(stop);
        }
        assert!(
            values.len() >= 2,
            "gradient requires at least 2 stops, got {}",
            values.len(),
        );
        Self(values)
    }
}

impl std::ops::Deref for GradientStops {
    type Target = [Stop];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl std::ops::DerefMut for GradientStops {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut_slice()
    }
}

impl std::hash::Hash for GradientStops {
    /// **Colour goes in the low half.** `ColorU8::to_u32` is
    /// `from_be_bytes([r, g, b, a])`, so red is its top byte; packing the
    /// colour into the *high* half of this word puts red at bit 56, and
    /// `FxHasher`'s `(hash + word) * K` propagates entropy upward only —
    /// red would get eight bits of spread and never reach the low bits
    /// hashbrown selects its bucket from. Offsets are almost always the
    /// constants 0 and 255, so that layout left the bucket index riding
    /// on the alpha and blue channels alone.
    ///
    /// The cost was not theoretical: a palette of 200 gradients sharing a
    /// blue channel and varying red/green — one hue family, an ordinary
    /// way to generate themed accents — hashed into a *single* bucket,
    /// turning `CpuGradientAtlas`'s index into one long probe chain
    /// (~230 ns per lookup against ~18 ns). Even unrelated hand-authored
    /// colours reached only a quarter of the available buckets.
    ///
    /// Swapping the halves carries exactly the same information and costs
    /// the same one write; it just puts the varying bytes where the
    /// multiply can spread them. Pinned by
    /// [`hash_spreads_across_buckets_for_structured_palettes`].
    ///
    /// [`hash_spreads_across_buckets_for_structured_palettes`]:
    ///     self::tests::hash_spreads_across_buckets_for_structured_palettes
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.len() as u8);
        for stop in self.iter() {
            state.write_u64((u64::from(stop.offset_u8) << 32) | u64::from(stop.color.to_u32()));
        }
    }
}

#[cfg(test)]
mod tests {
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
}
