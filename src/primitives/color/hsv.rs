//! HSV — the classic axes, kept so a number copied out of another tool still
//! means what it says.

use crate::primitives::color::{RgbaF32, linear_to_srgb};

/// Hue, saturation and value in the classic HSV space.
///
/// Every axis is `0..1`, and `h` wraps at `1`. **The axes are defined on
/// sRGB-encoded components**, which is what every other tool means by HSV and
/// what makes `v = 0.5` read as `#808080` rather than as half the light. The
/// conversion therefore goes through [`RgbaF32::srgb`], not
/// [`RgbaF32::new`].
///
/// It is the alternate model, not the default:
/// [`Okhsv`](crate::Okhsv) is the same three axes without HSV's hue
/// distortion and hue-dependent brightness. HSV is here because a colour
/// matched against Photoshop, Figma or a CSS value has to land on the same
/// numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsv {
    /// Hue around the colour circle, `0..1`. Wraps.
    pub h: f32,
    /// Saturation, `0` grey to `1` fully coloured.
    pub s: f32,
    /// Value, `0` black to `1` at full brightness.
    pub v: f32,
}

/// Channel spread below which a colour has no hue of its own.
const GREY_SPREAD: f32 = 1e-6;

impl Hsv {
    /// Construct from the three axes. Out-of-range values are the caller's
    /// until [`Self::to_color`], which wraps the hue and clamps the rest.
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    /// The opaque colour these axes name.
    pub fn to_color(self) -> RgbaF32 {
        let hue = self.h.rem_euclid(1.0) * 6.0;
        let sat = self.s.clamp(0.0, 1.0);
        let val = self.v.clamp(0.0, 1.0);
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
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let spread = high - low;
        if spread <= GREY_SPREAD {
            return Self {
                h: fallback_hue.rem_euclid(1.0),
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
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::hsv::Hsv;
    use crate::primitives::color::srgba_u8::SrgbaU8;

    /// The axes are sRGB-encoded, not linear. Half value on a pure hue is
    /// `#800000` — 128, the encoded midpoint. Reading the axes as linear
    /// would give 188, and the picker would paint a field nobody recognises.
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

    /// The six corners of the ramp are the six saturated cube corners.
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

    #[test]
    fn round_trip_holds_across_the_cube() {
        let mut worst = 0.0_f32;
        for hi in 0..9 {
            for si in 1..9 {
                for vi in 1..9 {
                    let start = Hsv::new(hi as f32 / 9.0, si as f32 / 8.0, vi as f32 / 8.0);
                    let back = Hsv::from_color(start.to_color(), start.h);
                    worst = worst
                        .max((back.h - start.h).abs())
                        .max((back.s - start.s).abs())
                        .max((back.v - start.v).abs());
                }
            }
        }
        assert!(worst < 1e-3, "worst axis drift {worst}");
    }

    #[test]
    fn grey_keeps_the_fallback_hue() {
        let coords = Hsv::from_color(RgbaF32::srgb(0.4, 0.4, 0.4), 0.618);
        assert_eq!(coords.h, 0.618);
        assert_eq!(coords.s, 0.0);
    }

    #[test]
    fn out_of_range_axes_wrap_and_clamp() {
        assert_eq!(
            Hsv::new(1.25, 2.0, 2.0).to_color().to_srgba_u8(),
            Hsv::new(0.25, 1.0, 1.0).to_color().to_srgba_u8(),
        );
    }
}
