//! HSV — the classic axes, kept so a number copied out of another tool still
//! means what it says.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgb_transfer;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Hue, saturation and value in the classic HSV space.
///
/// Every axis is `0..1`; `h` wraps. The axes are on sRGB-encoded components
/// (as in other tools), so conversion goes through [`RgbaF32::srgb`]. The
/// alternate model; [`Okhsv`](crate::Okhsv) is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsv {
    /// Hue.
    pub h: f32,
    /// Saturation.
    pub s: f32,
    /// Value.
    pub v: f32,
}

/// Channel spread below which a color has no hue of its own.
const GRAY_SPREAD: f32 = 1e-6;

impl Hsv {
    /// Out-of-range values are resolved by [`Self::to_color`]: hue wraps, the rest clamp, non-finite reads as `0`.
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    /// The color in linear RGB.
    pub fn to_color(self) -> RgbaF32 {
        let [r, g, b] = HsvColumn::new(self.h, self.s).encoded(self.v);
        RgbaF32::srgb(r, g, b)
    }

    /// The axes naming `color`, ignoring its alpha.
    ///
    /// `fallback_hue` answers gray, which has no hue to recover.
    pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self {
        let r = srgb_transfer::encode(color.r);
        let g = srgb_transfer::encode(color.g);
        let b = srgb_transfer::encode(color.b);
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let spread = high - low;
        if spread <= GRAY_SPREAD {
            return Self {
                h: domain::turn(fallback_hue),
                s: 0.0,
                v: high.clamp(0.0, 1.0),
            };
        }
        let sixth = if high == r {
            ((g - b) / spread).rem_euclid(6.0)
        } else if high == g {
            (b - r) / spread + 2.0
        } else {
            (r - g) / spread + 4.0
        };
        Self {
            h: (sixth / 6.0).rem_euclid(1.0),
            s: (spread / high).clamp(0.0, 1.0),
            v: high.clamp(0.0, 1.0),
        }
    }
}

/// One hue and saturation of [`Hsv`], ready to answer a run of values: each channel is the
/// value times a factor fixed here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HsvColumn {
    sector: u32,
    /// `1 - s`, `1 - f s` and `1 - (1 - f) s`, for the hue's place `f` within its sector.
    down: f32,
    falling: f32,
    rising: f32,
}

impl HsvColumn {
    /// The hue wraps and the saturation clamps, as [`Hsv::to_color`] reads them.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a turn puts the hue in [0, 6), so its sector is non-negative"
    )]
    pub(crate) fn new(h: f32, s: f32) -> Self {
        let hue = domain::turn(h) * 6.0;
        let sat = domain::fraction(s);
        let sector = hue.floor();
        let f = hue - sector;
        Self {
            sector: sector as u32 % 6,
            down: 1.0 - sat,
            falling: 1.0 - f * sat,
            rising: 1.0 - (1.0 - f) * sat,
        }
    }

    /// The sRGB-encoded channels at value `v`, which clamps.
    #[inline]
    fn encoded(self, v: f32) -> [f32; 3] {
        self.encoded_at(domain::fraction(v))
    }

    /// [`Self::encoded`] at the clamped value `val`.
    #[inline(always)]
    fn encoded_at(self, val: f32) -> [f32; 3] {
        let down = val * self.down;
        let falling = val * self.falling;
        let rising = val * self.rising;
        match self.sector {
            0 => [val, rising, down],
            1 => [falling, val, down],
            2 => [down, val, rising],
            3 => [down, falling, val],
            4 => [rising, down, val],
            _ => [val, down, falling],
        }
    }

    /// The texel of `encoded` channels, without the decode and encode between.
    #[inline(always)]
    fn texel([r, g, b]: [f32; 3]) -> SrgbaU8 {
        SrgbaU8 {
            r: srgb_transfer::byte_of_encoded(r),
            g: srgb_transfer::byte_of_encoded(g),
            b: srgb_transfer::byte_of_encoded(b),
            a: u8::MAX,
        }
    }
}

/// `N` columns of one hue side by side, a lane each, so a row of them is one loop. The hue fixes
/// the sector; only the factors vary along the row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HsvBlock<const N: usize> {
    sector: u32,
    down: [f32; N],
    falling: [f32; N],
    rising: [f32; N],
}

impl<const N: usize> HsvBlock<N> {
    /// Hue `h` at each saturation of `sats`, at most `N`, as [`HsvColumn::new`] reads them; the
    /// lanes past them hold saturation 0.
    pub(crate) fn new(h: f32, sats: impl IntoIterator<Item = f32>) -> Self {
        let gray = HsvColumn::new(h, 0.0);
        let mut block = Self {
            sector: gray.sector,
            down: [gray.down; N],
            falling: [gray.falling; N],
            rising: [gray.rising; N],
        };
        for (lane, s) in sats.into_iter().enumerate() {
            let column = HsvColumn::new(h, s);
            block.down[lane] = column.down;
            block.falling[lane] = column.falling;
            block.rising[lane] = column.rising;
        }
        block
    }

    /// Each lane's `Hsv::new(h, s, v).to_color().to_srgba8()`, into `out`, a texel a lane from
    /// the first.
    pub(crate) fn row(&self, v: f32, out: &mut [SrgbaU8]) {
        let val = domain::fraction(v);
        let lanes = self.down.iter().zip(&self.falling).zip(&self.rising);
        for (texel, ((&down, &falling), &rising)) in out.iter_mut().zip(lanes) {
            let column = HsvColumn {
                sector: self.sector,
                down,
                falling,
                rising,
            };
            *texel = HsvColumn::texel(column.encoded_at(val));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::hsv::Hsv;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;

    /// Axes are sRGB-encoded: half value on a pure hue is `#800000` (128), not 188.
    #[test]
    fn value_is_an_encoded_component() {
        assert_eq!(
            Hsv::new(0.0, 1.0, 0.5).to_color().to_srgba8(),
            SrgbaU8::hex(0x800000)
        );
        assert_eq!(
            Hsv::new(0.0, 0.0, 0.5).to_color().to_srgba8(),
            SrgbaU8::hex(0x808080)
        );
    }

    #[test]
    fn the_six_corners_are_the_cube_corners() {
        let want = [
            SrgbaU8::hex(0xff0000),
            SrgbaU8::hex(0xffff00),
            SrgbaU8::hex(0x00ff00),
            SrgbaU8::hex(0x00ffff),
            SrgbaU8::hex(0x0000ff),
            SrgbaU8::hex(0xff00ff),
        ];
        for (step, expected) in want.iter().enumerate() {
            let h = step as f32 / 6.0;
            assert_eq!(
                Hsv::new(h, 1.0, 1.0).to_color().to_srgba8(),
                *expected,
                "hue {h}"
            );
        }
    }
}
