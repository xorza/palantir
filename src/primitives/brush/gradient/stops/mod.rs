//! A gradient's colour stops: the u8-quantized stop itself, the inline
//! run a gradient carries them in, and the builder that sorts and
//! validates one.

use crate::primitives::color::ColorU8;
use crate::primitives::num;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
        Self {
            offset_u8: num::unit_to_u8(offset),
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

impl Serialize for Stop {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Stop", 2)?;
        state.serialize_field("offset", &self.offset())?;
        state.serialize_field("color", &self.color)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Stop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct RawStop {
            offset: f32,
            color: ColorU8,
        }

        let raw = RawStop::deserialize(deserializer)?;
        if !raw.offset.is_finite() {
            return Err(D::Error::custom("gradient stop offset must be finite"));
        }
        Ok(Stop::new(raw.offset, raw.color))
    }
}

/// Inline gradient-stop sequence whose length is always two through
/// eight, **held in ascending offset order**.
///
/// The ordering is an invariant of the type, not a step the bake does,
/// because this value *is* the gradient's cache identity: `Eq`/`Hash`
/// run over the raw array, so two sequences differing only in the order
/// they were written hash apart while baking a byte-identical LUT row —
/// two atlas rows, two bakes and two eviction slots for one gradient.
/// Sorting at construction makes identity and bake agree by
/// construction. That is also why there is no `DerefMut`: handing out
/// `&mut [Stop]` would let a caller reorder the offsets afterwards and
/// put the two back out of step.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientStops(ArrayVec<[Stop; MAX_STOPS]>);

impl GradientStops {
    /// Collect stops into inline storage, panicking on an invalid count.
    ///
    /// Sorted on the way in; equal offsets keep their written order, so a
    /// hard colour break authored as two stops at the same position still
    /// reads in the direction it was written.
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        let mut builder = GradientStopsBuilder::default();
        for stop in stops {
            builder.push(stop);
        }
        builder.build()
    }

    /// The one place a `GradientStops` is built, and so the one place its
    /// ascending-offset invariant is established.
    ///
    /// Both doors end here — [`GradientStopsBuilder::build`], which every
    /// authored gradient goes through, and the `Deserialize` impl below —
    /// because they differ only in how they *reject* a bad stop count, a
    /// panic against a deserialization error, and not at all in what a
    /// good one has to become. Sorting at each of them instead left
    /// the deserializer holding the wire order, so the invariant the type
    /// doc states held for authored gradients alone.
    ///
    /// Insertion sort: `MAX_STOPS` is 8 and the input is nearly always
    /// already ordered, so this is a comparison pass and no swaps. Stable,
    /// because the compare is a strict `>` — equal offsets keep the order
    /// they were written in.
    fn sorted(mut values: ArrayVec<[Stop; MAX_STOPS]>) -> Self {
        for index in 1..values.len() {
            let mut current = index;
            while current > 0 && values[current - 1].offset_u8 > values[current].offset_u8 {
                values.swap(current - 1, current);
                current -= 1;
            }
        }
        Self(values)
    }
}

/// The accumulating half of [`GradientStops`], and the one place the
/// `MAX_STOPS` capacity rule lives.
///
/// [`GradientStops::new`] fills one from an iterator; a
/// [`GradientBuilder`](crate::primitives::brush::gradient::gradient_builder::GradientBuilder)
/// holds one across its chained `stop` calls, so a ninth stop panics at
/// the call that wrote it rather than at `build`.
#[derive(Clone, Debug, Default)]
pub(crate) struct GradientStopsBuilder(ArrayVec<[Stop; MAX_STOPS]>);

impl GradientStopsBuilder {
    /// Append one stop, rejecting a ninth.
    pub(crate) fn push(&mut self, stop: Stop) {
        assert!(
            self.0.len() < MAX_STOPS,
            "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}",
        );
        self.0.push(stop);
    }

    /// Finish, requiring at least two stops.
    pub(crate) fn build(self) -> GradientStops {
        assert!(
            self.0.len() >= 2,
            "gradient requires at least 2 stops, got {}",
            self.0.len(),
        );
        GradientStops::sorted(self.0)
    }
}

impl std::ops::Deref for GradientStops {
    type Target = [Stop];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
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
    /// `tests::hash_spreads_across_buckets_for_structured_palettes`.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.len() as u8);
        for stop in self.iter() {
            state.write_u64((u64::from(stop.offset_u8) << 32) | u64::from(stop.color.to_u32()));
        }
    }
}

impl Serialize for GradientStops {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GradientStops {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = ArrayVec::<[Stop; MAX_STOPS]>::deserialize(deserializer)?;
        if values.len() < 2 {
            return Err(D::Error::custom(format_args!(
                "gradient requires at least 2 stops, got {}",
                values.len(),
            )));
        }
        Ok(Self::sorted(values))
    }
}

#[cfg(test)]
mod tests;
