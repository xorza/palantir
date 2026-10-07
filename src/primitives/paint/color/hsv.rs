//! HSV — the classic axes, kept so a number copied out of another tool still
//! means what it says.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgb_transfer;

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

/// Channel spread below which a colour has no hue of its own.
const GREY_SPREAD: f32 = 1e-6;

impl Hsv {
    /// Out-of-range values are resolved by [`Self::to_color`]: hue wraps, the rest clamp, non-finite reads as `0`.
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "a turn puts the hue in [0, 6), so its sector is non-negative"
    )]
    /// The colour in linear RGB.
    pub fn to_color(self) -> RgbaF32 {
        let hue = domain::turn(self.h) * 6.0;
        let sat = domain::fraction(self.s);
        let val = domain::fraction(self.v);
        let sector = hue.floor();
        let f = hue - sector;
        let down = val * (1.0 - sat);
        let falling = val * (1.0 - f * sat);
        let rising = val * (1.0 - (1.0 - f) * sat);
        let [r, g, b] = match sector as u32 % 6 {
            0 => [val, rising, down],
            1 => [falling, val, down],
            2 => [down, val, rising],
            3 => [down, falling, val],
            4 => [rising, down, val],
            _ => [val, down, falling],
        };
        RgbaF32::srgb(r, g, b)
    }

    /// The axes naming `color`, ignoring its alpha.
    ///
    /// `fallback_hue` answers grey, which has no hue to recover.
    pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self {
        let r = srgb_transfer::encode(color.r);
        let g = srgb_transfer::encode(color.g);
        let b = srgb_transfer::encode(color.b);
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let spread = high - low;
        if spread <= GREY_SPREAD {
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

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::hsv::Hsv;
    use crate::primitives::paint::color::srgba_u8::SrgbaU8;

    /// Axes are sRGB-encoded: half value on a pure hue is `#800000` (128), not 188.
    #[test]
    fn value_is_an_encoded_component() {
        assert_eq!(
            Hsv::new(0.0, 1.0, 0.5).to_color().to_srgba_u8(),
            SrgbaU8::hex(0x800000)
        );
        assert_eq!(
            Hsv::new(0.0, 0.0, 0.5).to_color().to_srgba_u8(),
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
                Hsv::new(h, 1.0, 1.0).to_color().to_srgba_u8(),
                *expected,
                "hue {h}"
            );
        }
    }
}
