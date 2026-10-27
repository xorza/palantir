//! [`Oklab`]: Björn Ottosson's perceptual color space, and its conversions to and from linear
//! RGB.

use crate::primitives::paint::color::RgbaF32;

/// A color in Oklab: perceived lightness `l`, roughly `0..1`, and the green–red `a` and
/// blue–yellow `b` axes, roughly `-0.5..0.5`. Matrix constants from Ottosson's reference
/// (<https://bottosson.github.io/posts/oklab/>).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Oklab {
    pub(crate) l: f32,
    pub(crate) a: f32,
    pub(crate) b: f32,
}

impl Oklab {
    /// `color` in Oklab, ignoring alpha, as [`Hsv::from_color`](crate::Hsv::from_color) does.
    #[inline]
    pub(crate) fn from_color(color: RgbaF32) -> Self {
        let RgbaF32 { r, g, b, .. } = color;
        let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();
        Self {
            l: 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
            a: 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
            b: 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
        }
    }

    /// This color, opaque, the inverse of [`Self::from_color`], as [`Hsv::to_color`](crate::Hsv::to_color) is. The
    /// intermediate cubes can go negative for an out-of-gamut color, and so can a channel;
    /// interpolating between in-gamut colors stays in gamut.
    #[inline]
    pub(crate) fn to_color(self) -> RgbaF32 {
        let Self { l, a, b } = self;
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
        let l3 = l_ * l_ * l_;
        let m3 = m_ * m_ * m_;
        let s3 = s_ * s_ * s_;
        RgbaF32::new(
            4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
            -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3,
            -0.004_196_086_4 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
            1.0,
        )
    }

    /// Each coordinate times `k`.
    #[inline]
    pub(crate) fn scale(self, k: f32) -> Self {
        Self {
            l: self.l * k,
            a: self.a * k,
            b: self.b * k,
        }
    }

    /// Each coordinate over `d`.
    #[inline]
    pub(crate) fn divide(self, d: f32) -> Self {
        Self {
            l: self.l / d,
            a: self.a / d,
            b: self.b / d,
        }
    }

    /// From `self` toward `to` by `t`, coordinate by coordinate.
    #[inline]
    pub(crate) fn lerp(self, to: Self, t: f32) -> Self {
        Self {
            l: self.l + (to.l - self.l) * t,
            a: self.a + (to.a - self.a) * t,
            b: self.b + (to.b - self.b) * t,
        }
    }
}
