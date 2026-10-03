//! Linear RGB in four f16 lanes: the lowered colour every draw lane carries.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Linear-RGB colour packed as four f16 lanes in 8 B (align 2).
/// Same lane scheme as `Corners` — pack and unpack go through
/// `F16x4::from_lanes` and `F16x4::lanes`, one SIMD instruction on
/// targets with hardware f16 support and a scalar walk otherwise. f16
/// carries ~3 decimal digits and the full f32 range — well below
/// display quantization.
///
/// Use this for storage sites that want half the footprint of
/// `RgbaF32` (16 B) at display precision. Pod-compatible; Hash delegates
/// to [`F16x4`] (one `u64` write).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RgbaF16(F16x4);

impl RgbaF16 {
    pub const TRANSPARENT: Self = Self(F16x4::ZERO);

    /// Opaque white — the identity of the channel-by-channel multiply a
    /// colour lane applies to a ramp sample.
    pub(crate) const WHITE: Self = Self(F16x4::ONE);

    /// Linear channels and a straight alpha, packed to f16 lanes — the
    /// peer of [`RgbaF32::new`], with no encoding in its name for the
    /// same reason.
    #[inline]
    pub(crate) fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self(F16x4::from_lanes([r, g, b, a]))
    }

    /// Scale the alpha lane by `by`, leaving the colour lanes alone.
    ///
    /// What a paint animation's alpha channel folds into a draw. Storage
    /// is straight-alpha, so this is one lane and no premultiply
    /// rebalancing. `by == 1.0` is the identity and the common case, so
    /// the caller skips this rather than paying the unpack.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        let [r, g, b, a] = self.0.lanes();
        Self(F16x4::from_lanes([r, g, b, a * by]))
    }

    /// True when alpha is below `EPS` — paints nothing visible. Reuses
    /// [`F16x4::lane_is_noop`]'s bit-trick (mask sign, compare against
    /// the `EPS` pattern) so no f16→f32 conversion is needed.
    #[inline]
    pub const fn is_noop(self) -> bool {
        // A NaN in *any* lane, not just alpha: an opaque colour with a
        // NaN red channel reaches the shader and renders as
        // hardware-dependent garbage. Covering all four costs one
        // masked add, not three extra compares.
        self.0.lane_is_noop(3) || self.0.has_nan()
    }

    /// True when alpha is within `EPS` of 1.0 — paints with full
    /// coverage. Mirror of `is_noop` at the opposite end of the
    /// scale; same bit-trick, no f16→f32 conversion.
    ///
    /// On this tier alone, though all three colour types carry `is_noop`:
    /// occlusion pruning is what asks, it runs in the composer, and the
    /// composer sees lowered colour. Authoring colour is never asked
    /// whether it is opaque.
    #[inline]
    pub const fn is_opaque(self) -> bool {
        self.0.lane_is_opaque(3)
    }

    /// All four lanes unpacked to f32 at once. Single instruction on
    /// F16C/fp16 targets.
    #[inline]
    pub fn unpack(self) -> RgbaF32 {
        let [r, g, b, a] = self.0.lanes();
        RgbaF32 { r, g, b, a }
    }

    /// The 8 storage bytes as one `u64` — used by the record store's
    /// solid-fill payload packing where a `RgbaF16` rides in a `u64`
    /// slot alongside the gradient-hash alternative.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl From<RgbaF32> for RgbaF16 {
    /// Four-lane f32→f16 pack — single instruction on F16C/fp16
    /// targets, scalar fallback elsewhere.
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Self::new(c.r, c.g, c.b, c.a)
    }
}

impl From<SrgbaU8> for RgbaF16 {
    /// The exact decode, packed. Every byte survives the trip back: f16
    /// holds each decoded value well inside half a display step.
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
