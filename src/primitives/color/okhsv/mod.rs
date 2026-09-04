//! Okhsv — the picker's default axes, and the sRGB gamut solve behind them.

use crate::primitives::color::{RgbaF32, linear_to_oklab, oklab_to_linear};
use std::f32::consts::{PI, TAU};

/// Hue, saturation and value in Björn Ottosson's Okhsv space.
///
/// Every axis is `0..1`, and `h` wraps at `1`. `s = 1` is the sRGB gamut edge
/// and `v = 1` its brightest slice, so **every triple in the unit cube names a
/// colour inside the gamut** and no clipping step is needed.
///
/// Okhsv replaces [`Hsv`](crate::Hsv) axis for axis and fixes what HSV gets
/// wrong. HSV's hue distorts — deep blue drifts to purple as saturation moves
/// — and its `v` reads as a different brightness on every hue. Okhsv is built
/// on Oklab, so hue stays put while the other two axes move, and one `v`
/// reads as one brightness right around the circle.
///
/// The conversion is Ottosson's reference
/// (<https://bottosson.github.io/posts/colorpicker/>) over the crate's own
/// Oklab matrices. It works entirely in linear light, so it never pays the
/// cubic sRGB approximation that [`RgbaF32::rgb`] carries.
///
/// # The blue sliver
///
/// One small part of sRGB is unreachable: a wedge around pure blue. The gamut
/// is not star-shaped in Oklab there — sweeping chroma out along blue's hue,
/// red dips below zero, comes back, and only then does green leave. Okhsv's
/// gamut edge is the *first* crossing, so it stops short and `#0000ff` sits
/// just outside the cube. `Okhsv { s: 1.0, v: 1.0 }` at blue's hue is
/// `#0038ff`.
///
/// Every Okhsv picker has this, and it is a property of the space rather than
/// of this port. A picker answers it by leaving the other routes to a colour
/// open — the hex field, the channel values, and [`Hsv`](crate::Hsv) — and by
/// never rewriting a colour the user did not aim at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Okhsv {
    /// Hue around the Oklab circle, `0..1`. Wraps.
    pub h: f32,
    /// Saturation, `0` grey to `1` at the gamut edge.
    pub s: f32,
    /// Value, `0` black to `1` at the brightest slice of this hue.
    pub v: f32,
}

/// Chroma below which a colour has no hue of its own and the caller's
/// fallback answers instead. Well under one 8-bit step at any lightness.
const GREY_CHROMA: f32 = 1e-7;

/// The saturation the gamut triangle is anchored at in Ottosson's fit. Not a
/// tunable: the inverse below undoes exactly this constant.
const ANCHOR_S: f32 = 0.5;

/// The toe's shape, from the reference. `TOE_K3` is what makes `toe(1) == 1`.
const TOE_K1: f32 = 0.206;
const TOE_K2: f32 = 0.03;
const TOE_K3: f32 = (1.0 + TOE_K1) / (1.0 + TOE_K2);

/// Halley steps taken to land the cusp. See [`max_saturation`] for the
/// measurement that picked three.
const HALLEY_STEPS: usize = 3;

impl Okhsv {
    /// Construct from the three axes. Out-of-range values are the caller's
    /// until [`Self::to_color`], which wraps the hue and clamps the rest.
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    /// The opaque colour these axes name.
    ///
    /// Alpha is not an Okhsv axis, so the result is opaque and a caller that
    /// carries one applies it with [`RgbaF32::with_alpha`].
    pub fn to_color(self) -> RgbaF32 {
        Self::slice(self.h).color(self.s, self.v)
    }

    /// This hue's gamut geometry, solved once so a run of samples sharing a
    /// hue pays for it once.
    ///
    /// What a colour field is built from: every texel of one field shares the
    /// hue, and the cusp solve is the expensive half of the conversion.
    pub(crate) fn slice(hue: f32) -> OkhsvSlice {
        let (sin, cos) = (TAU * hue.rem_euclid(1.0)).sin_cos();
        let slopes = Cusp::find(cos, sin).slopes();
        OkhsvSlice {
            cos,
            sin,
            slopes,
            k: 1.0 - ANCHOR_S / slopes.s,
        }
    }

    /// The axes naming `color`, ignoring its alpha.
    ///
    /// `fallback_hue` answers grey, which has no hue to recover — without it
    /// a picker would lose the hue every time the value reached zero.
    pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self {
        let lab = linear_to_oklab(color.r, color.g, color.b);
        let lightness = lab[0];
        let chroma = lab[1].hypot(lab[2]);
        if chroma < GREY_CHROMA || lightness <= 0.0 {
            return Self {
                h: fallback_hue.rem_euclid(1.0),
                s: 0.0,
                v: toe(lightness).clamp(0.0, 1.0),
            };
        }
        let cos = lab[1] / chroma;
        let sin = lab[2] / chroma;
        let slopes = Cusp::find(cos, sin).slopes();

        // Undo `to_color` in the order it applied: triangle, then curved top,
        // then toe.
        let t = slopes.t / (chroma + lightness * slopes.t);
        let l_v = t * lightness;
        let c_v = t * chroma;
        let l_vt = toe_inv(l_v);
        let c_vt = c_v * l_vt / l_v;
        let scale = peak_scale([l_vt, cos * c_vt, sin * c_vt]);
        let unscaled = lightness / scale;
        let toed = toe(unscaled);

        let k = 1.0 - ANCHOR_S / slopes.s;
        Self {
            h: (0.5 + 0.5 * (-lab[2]).atan2(-lab[1]) / PI).rem_euclid(1.0),
            s: ((ANCHOR_S + slopes.t) * c_v / (slopes.t * ANCHOR_S + slopes.t * k * c_v))
                .clamp(0.0, 1.0),
            v: (toed / l_v).clamp(0.0, 1.0),
        }
    }
}

/// One hue's gamut geometry, hoisted out of a sampling loop.
///
/// Holds what [`Okhsv::to_color`] would otherwise re-solve per call: the hue's
/// direction in Oklab and the two edges of its gamut triangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OkhsvSlice {
    cos: f32,
    sin: f32,
    slopes: CuspSlopes,
    k: f32,
}

impl OkhsvSlice {
    /// The opaque colour at `s` and `v` on this hue. Both clamp to `0..1`.
    pub(crate) fn color(self, s: f32, v: f32) -> RgbaF32 {
        let sat = s.clamp(0.0, 1.0);
        let val = v.clamp(0.0, 1.0);

        // The gamut slice as a perfect triangle first: `l_v` / `c_v` are the
        // lightness and chroma at `v = 1`.
        let denom = ANCHOR_S + self.slopes.t - self.slopes.t * self.k * sat;
        let l_v = 1.0 - sat * ANCHOR_S / denom;
        let c_v = sat * self.slopes.t * ANCHOR_S / denom;

        // Then the two compensations that bend the triangle back onto the
        // real gamut: the toe on lightness, and the curved top.
        let l_vt = toe_inv(l_v);
        let c_vt = c_v * l_vt / l_v;
        let lightness = val * l_v;
        let chroma = val * c_v;
        let toed = toe_inv(lightness);
        let chroma = if lightness > 0.0 {
            chroma * toed / lightness
        } else {
            0.0
        };
        let scale = peak_scale([l_vt, self.cos * c_vt, self.sin * c_vt]);

        let rgb = oklab_to_linear([
            toed * scale,
            chroma * scale * self.cos,
            chroma * scale * self.sin,
        ]);
        // The edge of the gamut lands a hair outside it: the reference port
        // returns -1/255 on the red corner. Clamping here is what lets the
        // whole unit cube be called in-gamut.
        RgbaF32::new(
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
            1.0,
        )
    }
}

/// The most chromatic point of one hue slice: where the slice's edge turns
/// the corner of the sRGB cube.
#[derive(Clone, Copy, Debug)]
struct Cusp {
    l: f32,
    c: f32,
}

impl Cusp {
    /// The cusp of the hue whose Oklab direction is `(cos, sin)`, which must
    /// be a unit vector.
    fn find(cos: f32, sin: f32) -> Self {
        let s = max_saturation(cos, sin);
        let l = peak_scale([1.0, s * cos, s * sin]);
        Self { l, c: l * s }
    }

    /// The cusp as the two slopes the gamut triangle is drawn from: `s` up
    /// from black, `t` down from white.
    fn slopes(self) -> CuspSlopes {
        CuspSlopes {
            s: self.c / self.l,
            t: self.c / (1.0 - self.l),
        }
    }
}

/// A cusp expressed as the gamut triangle's two edges.
#[derive(Clone, Copy, Debug)]
struct CuspSlopes {
    s: f32,
    t: f32,
}

/// Greatest `C/L` this hue direction reaches inside sRGB.
///
/// A polynomial fit per cube face, then Halley steps — the reference's recipe,
/// run to convergence rather than once. Measured worst chroma error against a
/// converged solve, over 3600 hues: one step `3.2e-3`, two steps `2.3e-5`,
/// three steps `1.1e-11`. Three is therefore exact in `f32`, and it costs
/// forty flops on a conversion that only runs when the hue moves.
fn max_saturation(a: f32, b: f32) -> f32 {
    // Which channel goes negative first decides both the fit and the row of
    // the Oklab matrix the Halley step differentiates.
    let (k, w) = if -1.881_703_3 * a - 0.809_364_9 * b > 1.0 {
        (
            [
                1.190_862_8,
                1.765_767_3,
                0.596_626_4,
                0.755_152,
                0.567_712_4,
            ],
            [4.076_741_7, -3.307_711_6, 0.230_969_94],
        )
    } else if 1.814_441 * a - 1.194_452_8 * b > 1.0 {
        (
            [
                0.739_565_13,
                -0.459_544_03,
                0.082_854_27,
                0.125_410_7,
                0.145_032_03,
            ],
            [-1.268_438, 2.609_757_4, -0.341_319_4],
        )
    } else {
        (
            [
                1.357_336_5,
                -0.009_157_99,
                -1.151_302_1,
                -0.505_596_04,
                0.006_921_67,
            ],
            [-0.004_196_086_4, -0.703_418_6, 1.707_614_7],
        )
    };
    let mut s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;

    let k_l = 0.396_337_78 * a + 0.215_803_76 * b;
    let k_m = -0.105_561_346 * a - 0.063_854_17 * b;
    let k_s = -0.089_484_18 * a - 1.291_485_5 * b;

    for _ in 0..HALLEY_STEPS {
        let l_ = 1.0 + s * k_l;
        let m_ = 1.0 + s * k_m;
        let s_ = 1.0 + s * k_s;
        let l3 = l_ * l_ * l_;
        let m3 = m_ * m_ * m_;
        let s3 = s_ * s_ * s_;
        let d_l = 3.0 * k_l * l_ * l_;
        let d_m = 3.0 * k_m * m_ * m_;
        let d_s = 3.0 * k_s * s_ * s_;
        let dd_l = 6.0 * k_l * k_l * l_;
        let dd_m = 6.0 * k_m * k_m * m_;
        let dd_s = 6.0 * k_s * k_s * s_;

        let f = w[0] * l3 + w[1] * m3 + w[2] * s3;
        let f1 = w[0] * d_l + w[1] * d_m + w[2] * d_s;
        let f2 = w[0] * dd_l + w[1] * dd_m + w[2] * dd_s;
        s -= f * f1 / (f1 * f1 - 0.5 * f * f2);
    }
    s
}

/// The cube-root scale that brings `lab`'s brightest linear channel to
/// exactly one: what pins a slice's cusp, and its curved top, onto the real
/// gamut. One function for the cusp and for both conversion directions,
/// which is what keeps the two directions exact inverses of one another.
fn peak_scale(lab: [f32; 3]) -> f32 {
    let rgb = oklab_to_linear(lab);
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    debug_assert!(peak > 0.0, "a hue slice always has a positive peak");
    (1.0 / peak).cbrt()
}

/// Oklab lightness → Okhsv's perceptual lightness. The reference's toe: it
/// pulls the darks apart so a value step is one step to the eye down there
/// too.
fn toe(x: f32) -> f32 {
    0.5 * (TOE_K3 * x - TOE_K1
        + ((TOE_K3 * x - TOE_K1) * (TOE_K3 * x - TOE_K1) + 4.0 * TOE_K2 * TOE_K3 * x).sqrt())
}

/// Inverse of [`toe`], in closed form.
fn toe_inv(x: f32) -> f32 {
    (x * x + TOE_K1 * x) / (TOE_K3 * (x + TOE_K2))
}

#[cfg(test)]
mod tests;
