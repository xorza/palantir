//! Linear RGB in four f16 lanes: the lowered color every draw lane carries.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;

/// Linear-RGB color packed as four f16 lanes in 8 B (align 2), half of `RgbaF32` at display precision (f16 carries ~3 decimal digits and magnitudes up to 65504, the bound [`domain::color`](crate::primitives::math::domain::color) holds a color to). Packs through `F16x4::from_lanes` / `lanes`; Pod-compatible; Hash delegates to [`F16x4`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RgbaF16(F16x4);

impl RgbaF16 {
    pub(crate) const TRANSPARENT: Self = Self(F16x4::ZERO);

    /// Opaque white: the identity of the channel-wise multiply a color lane applies to a ramp sample.
    pub(crate) const WHITE: Self = Self(F16x4::ONE);

    /// Linear channels and a straight alpha packed to f16 lanes; the peer of [`RgbaF32::new`].
    ///
    /// The one place a color is lowered, so the one clamp: a tween that overshoots (`OutBack`, a
    /// bouncy spring) may carry alpha past `0..=1` or a channel below zero, which the blend would
    /// paint as light added or taken away. Channels above 1 stay, as HDR. A NaN passes through,
    /// for [`Self::is_noop`] to drop.
    #[inline]
    pub(crate) fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        let packed = F16x4::from_lanes([
            at_least_zero(r),
            at_least_zero(g),
            at_least_zero(b),
            if a > 1.0 { 1.0 } else { at_least_zero(a) },
        ]);
        debug_assert!(
            !packed.any_lane_infinite(),
            "a color channel packs past the largest f16"
        );
        Self(packed)
    }

    /// Scale the alpha lane by `by`, leaving the color lanes alone (storage is straight-alpha, so no premultiply). `by == 1.0` is the common case, so callers skip this.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        let [r, g, b, a] = self.0.lanes();
        Self::new(r, g, b, a * by)
    }

    /// True when alpha is at most `EPS`, via [`F16x4::lane_is_noop`]'s bit-trick (no f16→f32 conversion).
    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        // A NaN in any lane, not just alpha, reaches the shader as garbage; covering all four costs one masked add.
        self.0.lane_is_noop(3) || self.0.has_nan()
    }

    /// True when alpha is within `EPS` of 1.0 (the mirror of `is_noop`, same bit-trick). On this tier alone: occlusion pruning asks, in the composer, which sees lowered color.
    #[inline]
    pub(crate) const fn is_opaque(self) -> bool {
        self.0.lane_is_opaque(3)
    }

    /// The 8 storage bytes as one `u64`, for the record store's solid-fill payload slot.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

/// `v`, or zero when it is negative; a NaN is kept, where `f32::max` would drop it.
#[inline]
fn at_least_zero(v: f32) -> f32 {
    if v < 0.0 { 0.0 } else { v }
}

impl From<RgbaF16> for RgbaF32 {
    /// All four lanes unpacked to f32 at once; every f16 is an f32.
    #[inline]
    fn from(c: RgbaF16) -> Self {
        let [r, g, b, a] = c.0.lanes();
        Self { r, g, b, a }
    }
}

impl NanCheck for RgbaF16 {
    #[inline]
    fn has_nan(&self) -> bool {
        self.0.has_nan()
    }
}
