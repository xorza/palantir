//! A gradient's colour stops: the quantized stop, the inline run, and its builder.

use crate::primitives::math::domain;
use crate::primitives::math::num;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash;
use std::ops;
use tinyvec::ArrayVec;

/// Hard cap on stops in a single gradient.
pub(crate) const MAX_STOPS: usize = 8;

/// One colour stop: a 0..1 offset quantized to 8 bits and an 8-bit **sRGB-encoded**
/// colour, so a hex-authored stop keeps its exact bytes. 5 B per stop;
/// storage-only, never animated. The quantization is private; serde carries the
/// decoded forms (`offset: 0.5`, `color: "#22ccdd"`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stop {
    offset_u8: u8,
    color: SrgbaU8,
}

impl Stop {
    /// Construct a stop. `offset` is a *fraction*, coerced (clamped to 0..=1,
    /// non-finite read as 0). `color` is a *colour*.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [colour](crate::widget::domain::color).
    #[inline]
    #[track_caller]
    pub fn new(offset: f32, color: RgbaF32) -> Self {
        Self {
            offset_u8: num::unit_to_u8(domain::fraction(offset)),
            color: domain::color(color).into(),
        }
    }

    /// The position decoded to a 0..1 f32.
    #[inline]
    pub const fn offset(self) -> f32 {
        self.offset_u8 as f32 / 255.0
    }

    /// The colour decoded to linear f32.
    #[inline]
    pub const fn color(self) -> RgbaF32 {
        RgbaF32::from_srgba(self.color)
    }
}

impl Serialize for Stop {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Stop", 2)?;
        state.serialize_field("offset", &self.offset())?;
        state.serialize_field("color", &self.color())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Stop {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct RawStop {
            offset: f32,
            color: RgbaF32,
        }

        let raw = RawStop::deserialize(deserializer)?;
        if !domain::is_fraction(raw.offset) {
            return Err(D::Error::custom(format_args!(
                "{}, got {}",
                domain::FRACTION_RULE,
                raw.offset
            )));
        }
        if !domain::is_color(raw.color) {
            return Err(D::Error::custom(domain::COLOR_RULE));
        }
        Ok(Stop::new(raw.offset, raw.color))
    }
}

/// Inline gradient-stop sequence of two to eight stops, **held in ascending offset
/// order**.
///
/// The order is an invariant because this value is the gradient's cache identity:
/// `Eq`/`Hash` run over the raw array, so unsorted duplicates would hash apart yet
/// bake one LUT row. Hence no `DerefMut`.
///
/// A `u8` count beside a fixed array (not `ArrayVec`'s `u16`) keeps
/// `LinearGradient` at 48 B. Slots past `len` hold `Stop::default()` so `Eq` agrees
/// with the live-stops-only `Hash`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientStops {
    len: u8,
    stops: [Stop; MAX_STOPS],
}

impl GradientStops {
    /// Collect stops into inline storage; panics on an invalid count. Sorted on the
    /// way in, equal offsets keeping written order.
    pub fn new(stops: impl IntoIterator<Item = Stop>) -> Self {
        let mut builder = GradientStopsBuilder::default();
        for stop in stops {
            builder.push(stop);
        }
        builder.build()
    }

    pub(crate) const fn as_slice(&self) -> &[Stop] {
        self.stops.split_at(self.len as usize).0
    }

    pub(crate) fn is_ascending(&self) -> bool {
        self.windows(2).all(|w| w[0].offset_u8 <= w[1].offset_u8)
    }

    /// The one place a `GradientStops` is built, so the ascending invariant is
    /// established here for both [`GradientStopsBuilder::build`] and `Deserialize`.
    /// Stable insertion sort: input is nearly ordered and the strict `>` keeps
    /// equal offsets in written order.
    fn sorted(mut values: ArrayVec<[Stop; MAX_STOPS]>) -> Self {
        for index in 1..values.len() {
            let mut current = index;
            while current > 0 && values[current - 1].offset_u8 > values[current].offset_u8 {
                values.swap(current - 1, current);
                current -= 1;
            }
        }
        let mut stops = [Stop::default(); MAX_STOPS];
        stops[..values.len()].copy_from_slice(&values);
        Self {
            len: values.len() as u8,
            stops,
        }
    }
}

/// The accumulating half of [`GradientStops`] and the one place the `MAX_STOPS`
/// rule lives; a gradient builder holds one so a ninth stop panics at its call.
#[derive(Clone, Debug, Default)]
pub(crate) struct GradientStopsBuilder(ArrayVec<[Stop; MAX_STOPS]>);

impl GradientStopsBuilder {
    pub(crate) fn push(&mut self, stop: Stop) {
        assert!(
            self.0.len() < MAX_STOPS,
            "gradient stop count exceeds MAX_STOPS = {MAX_STOPS}",
        );
        self.0.push(stop);
    }

    pub(crate) fn build(self) -> GradientStops {
        assert!(
            self.0.len() >= 2,
            "gradient requires at least 2 stops, got {}",
            self.0.len(),
        );
        GradientStops::sorted(self.0)
    }
}

impl ops::Deref for GradientStops {
    type Target = [Stop];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl hash::Hash for GradientStops {
    /// **Colour goes in the low half.** `SrgbaU8::to_u32` puts red on top and
    /// `FxHasher` spreads entropy upward only, so red in the high half never
    /// reaches the bucket-selecting low bits (200 gradients once collapsed to one
    /// bucket). Pinned by
    /// `tests::hash_spreads_across_buckets_for_structured_palettes`.
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.len() as u8);
        for stop in self.iter() {
            state.write_u64((u64::from(stop.offset_u8) << 32) | u64::from(stop.color.to_u32()));
        }
    }
}

impl Serialize for GradientStops {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
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
