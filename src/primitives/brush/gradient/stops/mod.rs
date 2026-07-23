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
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.len() as u8);
        for stop in self.iter() {
            state.write_u64(((stop.color.to_u32() as u64) << 32) | u64::from(stop.offset_u8));
        }
    }
}
