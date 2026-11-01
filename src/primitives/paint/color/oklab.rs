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
    /// `f32::cbrt` keeps the sign, so an out-of-gamut color with a negative cone response goes
    /// there and back.
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
    /// intermediate cubes can go negative for an out-of-gamut color, and so can a channel. A
    /// straight line between two in-gamut colors can leave the gamut: red to white peaks 11 %
    /// over in red at `t = 0.55`.
    #[inline]
    pub(crate) fn to_color(self) -> RgbaF32 {
        cone_color(self.cone_roots())
    }

    /// The cube roots of the cone responses, Ottosson's `l_`, `m_` and `s_`: each is 1 for white
    /// and in `0..=1` for a color in gamut. Linear in the coordinates, so scaling them scales the
    /// roots.
    #[inline]
    pub(crate) fn cone_roots(self) -> [f32; 3] {
        let Self { l, a, b } = self;
        [
            l + 0.396_337_78 * a + 0.215_803_76 * b,
            l - 0.105_561_346 * a - 0.063_854_17 * b,
            l - 0.089_484_18 * a - 1.291_485_5 * b,
        ]
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
}

/// The opaque linear color whose cone responses have the cube roots `roots`, the second half of
/// [`Oklab::to_color`].
#[inline]
pub(crate) fn cone_color([l_, m_, s_]: [f32; 3]) -> RgbaF32 {
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
