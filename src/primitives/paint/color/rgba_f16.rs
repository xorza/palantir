//! Linear RGB in four f16 lanes: the lowered colour every draw lane carries.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Linear-RGB colour packed as four f16 lanes in 8 B (align 2), half of `RgbaF32` at display precision (f16 carries ~3 decimal digits and the full f32 range). Packs through `F16x4::from_lanes` / `lanes`; Pod-compatible; Hash delegates to [`F16x4`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RgbaF16(F16x4);

impl RgbaF16 {
    pub(crate) const TRANSPARENT: Self = Self(F16x4::ZERO);

    /// Opaque white: the identity of the channel-wise multiply a colour lane applies to a ramp sample.
    pub(crate) const WHITE: Self = Self(F16x4::ONE);

    /// Linear channels and a straight alpha packed to f16 lanes; the peer of [`RgbaF32::new`].
    #[inline]
    pub(crate) fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self(F16x4::from_lanes([r, g, b, a]))
    }

    /// Scale the alpha lane by `by`, leaving the colour lanes alone (storage is straight-alpha, so no premultiply). `by == 1.0` is the common case, so callers skip this.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        let [r, g, b, a] = self.0.lanes();
        Self(F16x4::from_lanes([r, g, b, a * by]))
    }

    /// True when alpha is below `EPS`, via [`F16x4::lane_is_noop`]'s bit-trick (no f16→f32 conversion).
    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        // A NaN in any lane, not just alpha, reaches the shader as garbage; covering all four costs one masked add.
        self.0.lane_is_noop(3) || self.0.has_nan()
    }

    /// True when alpha is within `EPS` of 1.0 (the mirror of `is_noop`, same bit-trick). On this tier alone: occlusion pruning asks, in the composer, which sees lowered colour.
    #[inline]
    pub(crate) const fn is_opaque(self) -> bool {
        self.0.lane_is_opaque(3)
    }

    /// All four lanes unpacked to f32 at once.
    #[inline]
    pub(crate) fn unpack(self) -> RgbaF32 {
        let [r, g, b, a] = self.0.lanes();
        RgbaF32 { r, g, b, a }
    }

    /// The 8 storage bytes as one `u64`, for the record store's solid-fill payload slot.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl From<RgbaF32> for RgbaF16 {
    /// Four-lane f32→f16 pack.
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Self::new(c.r, c.g, c.b, c.a)
    }
}

impl From<SrgbaU8> for RgbaF16 {
    /// The exact decode, packed; every byte survives the round trip.
    #[inline]
    fn from(bytes: SrgbaU8) -> Self {
        RgbaF32::from_srgba(bytes).into()
    }
}

impl NanCheck for RgbaF16 {
    #[inline]
    fn has_nan(&self) -> bool {
        self.0.has_nan()
    }
}
