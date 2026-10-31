//! Okhsv: the picker's default axes and the sRGB gamut solve behind them.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::oklab::Oklab;
use crate::primitives::paint::color::srgb_transfer;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use std::f32::consts::TAU;

/// Hue, saturation and value in Björn Ottosson's Okhsv space
/// (<https://bottosson.github.io/posts/colorpicker/>), in linear light.
///
/// Every axis is `0..1` and `h` wraps. `s = 1` is the sRGB gamut edge and `v = 1`
/// its brightest slice, so **every triple in the unit cube is in gamut**. Unlike
/// [`Hsv`](crate::Hsv), hue stays put as the other axes move.
///
/// # The blue sliver
///
/// A wedge around pure blue is unreachable: Okhsv's gamut edge is the *first*
/// crossing, so `#0000ff` sits just outside the cube (`s = 1, v = 1` at blue's hue
/// is `#0037ff`). Keep hex, channels and [`Hsv`](crate::Hsv) available in a picker.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Okhsv {
    /// Hue around the Oklab circle, `0..1`. Wraps.
    pub h: f32,
    /// Saturation, `0` gray to `1` at the gamut edge.
    pub s: f32,
    /// Value, `0` black to `1` at the brightest slice of this hue.
    pub v: f32,
}

/// Chroma below which a color has no hue and the caller's fallback answers: `f32`
/// rounding leaves an exact gray up to ~`3e-7`, one byte from gray is at least
/// ~`1e-3`.
const GRAY_CHROMA: f32 = 1e-5;

/// The saturation the gamut triangle is anchored at in Ottosson's fit; the inverse
/// undoes this constant.
const ANCHOR_S: f32 = 0.5;

/// The toe's shape, from the reference; `TOE_K3` makes `toe(1) == 1`.
const TOE_K1: f32 = 0.206;
const TOE_K2: f32 = 0.03;
const TOE_K3: f32 = (1.0 + TOE_K1) / (1.0 + TOE_K2);

/// Halley steps to land the cusp; see [`max_saturation`].
const HALLEY_STEPS: usize = 3;

impl Okhsv {
    /// Construct from the three axes. [`Self::to_color`] wraps the hue, clamps the
    /// rest and reads a non-finite axis as `0`.
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    /// The opaque color these axes name; apply alpha with [`RgbaF32::with_alpha`].
    pub fn to_color(self) -> RgbaF32 {
        Self::slice(self.h).color(self.s, self.v)
    }

    pub(crate) fn slice(hue: f32) -> OkhsvSlice {
        let (sin, cos) = (TAU * domain::turn(hue)).sin_cos();
        OkhsvSlice::from_direction(cos, sin)
    }

    /// The axes naming `color`, ignoring alpha; `fallback_hue` answers gray, which
    /// has no hue.
    pub fn from_color(color: RgbaF32, fallback_hue: f32) -> Self {
        let lab = Oklab::from_color(color);
        let lightness = lab.l;
        let chroma = lab.a.hypot(lab.b);
        if chroma < GRAY_CHROMA || lightness <= 0.0 {
            return Self {
                h: domain::turn(fallback_hue),
                s: 0.0,
                v: toe(lightness).clamp(0.0, 1.0),
            };
        }
        let slice = OkhsvSlice::from_direction(lab.a / chroma, lab.b / chroma);
        let CuspSlopes { t, .. } = slice.slopes;

        let at_top = t / (chroma + lightness * t);
        let l_v = at_top * lightness;
        let c_v = at_top * chroma;
        let toed = toe(lightness / slice.top_scale(l_v, c_v));

        Self {
            h: (lab.b.atan2(lab.a) / TAU).rem_euclid(1.0),
            s: ((ANCHOR_S + t) * c_v / (t * ANCHOR_S + t * slice.k * c_v)).clamp(0.0, 1.0),
            v: (toed / l_v).clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OkhsvSlice {
    cos: f32,
    sin: f32,
    slopes: CuspSlopes,
    k: f32,
}

impl OkhsvSlice {
    fn from_direction(cos: f32, sin: f32) -> Self {
        let slopes = Cusp::find(cos, sin).slopes();
        Self {
            cos,
            sin,
            slopes,
            k: 1.0 - ANCHOR_S / slopes.s,
        }
    }

    /// Scale bending the triangle's top at the `v = 1` point `(l_v, c_v)` onto the
    /// real gamut; shared so the two directions stay exact inverses.
    fn top_scale(self, l_v: f32, c_v: f32) -> f32 {
        let l_vt = toe_inv(l_v);
        let c_vt = c_v * l_vt / l_v;
        peak_scale(Oklab {
            l: l_vt,
            a: self.cos * c_vt,
            b: self.sin * c_vt,
        })
    }

    /// The opaque color at `s` and `v` on this hue; both clamp to `0..1`.
    pub(crate) fn color(self, s: f32, v: f32) -> RgbaF32 {
        self.column(s).color(v)
    }

    /// This hue at saturation `s`, which clamps, with what every value shares solved once.
    pub(crate) fn column(self, s: f32) -> OkhsvColumn {
        let sat = domain::fraction(s);

        let denom = ANCHOR_S + self.slopes.t - self.slopes.t * self.k * sat;
        let l_v = 1.0 - sat * ANCHOR_S / denom;
        let c_v = sat * self.slopes.t * ANCHOR_S / denom;
        OkhsvColumn {
            cos: self.cos,
            sin: self.sin,
            l_v,
            c_v,
            scale: self.top_scale(l_v, c_v),
        }
    }

    /// This hue at each saturation of `sats`, at most `N`, which clamp; the lanes past them are
    /// black.
    pub(crate) fn block<const N: usize>(
        self,
        sats: impl IntoIterator<Item = f32>,
    ) -> OkhsvBlock<N> {
        let mut block = OkhsvBlock {
            cos: self.cos,
            sin: self.sin,
            l_v: [0.0; N],
            c_v: [0.0; N],
            scale: [0.0; N],
        };
        for (lane, s) in sats.into_iter().enumerate() {
            let column = self.column(s);
            block.l_v[lane] = column.l_v;
            block.c_v[lane] = column.c_v;
            block.scale[lane] = column.scale;
        }
        block
    }
}

/// One hue and saturation of [`Okhsv`], ready to answer a run of values: the triangle's top and
/// the gamut scale depend on the saturation alone. Take one from [`OkhsvSlice::column`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct OkhsvColumn {
    cos: f32,
    sin: f32,
    l_v: f32,
    c_v: f32,
    scale: f32,
}

impl OkhsvColumn {
    /// The opaque color at value `v`, which clamps.
    #[inline]
    pub(crate) fn color(self, v: f32) -> RgbaF32 {
        let [r, g, b] = self.rgb_at(domain::fraction(v));
        RgbaF32::new(r, g, b, 1.0)
    }

    /// [`Self::color`]'s channels at the clamped value `val`; branch-free, so a loop over
    /// lanes vectorizes.
    #[inline(always)]
    fn rgb_at(self, val: f32) -> [f32; 3] {
        let lightness = val * self.l_v;
        let chroma = val * self.c_v;
        let toed = toe_inv(lightness);
        let chroma = if lightness > 0.0 {
            chroma * toed / lightness
        } else {
            0.0
        };
        let scale = self.scale;

        let rgb = Oklab {
            l: toed * scale,
            a: chroma * scale * self.cos,
            b: chroma * scale * self.sin,
        }
        .to_color();
        // The gamut edge lands a hair outside (the reference gives -1/255 on red);
        // clamping makes the whole cube in-gamut.
        [
            rgb.r.clamp(0.0, 1.0),
            rgb.g.clamp(0.0, 1.0),
            rgb.b.clamp(0.0, 1.0),
        ]
    }
}

/// `N` columns of one hue side by side, a lane each, so a row of them is one vectorized loop.
/// Take one from [`OkhsvSlice::block`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct OkhsvBlock<const N: usize> {
    cos: f32,
    sin: f32,
    l_v: [f32; N],
    c_v: [f32; N],
    scale: [f32; N],
}

impl<const N: usize> OkhsvBlock<N> {
    /// Each lane's `OkhsvColumn::color(v).to_srgba8()`, into `out`, a texel a lane from the
    /// first.
    pub(crate) fn row(&self, v: f32, out: &mut [SrgbaU8]) {
        let [r, g, b] = self.rgb(domain::fraction(v));
        for (lane, texel) in out.iter_mut().enumerate() {
            *texel = SrgbaU8 {
                r: srgb_transfer::encode_byte(r[lane]),
                g: srgb_transfer::encode_byte(g[lane]),
                b: srgb_transfer::encode_byte(b[lane]),
                a: u8::MAX,
            };
        }
    }

    /// Every lane's channels at the clamped value `val`, as `[r, g, b]` lanes.
    fn rgb(&self, val: f32) -> [[f32; N]; 3] {
        let mut rgb = [[0.0; N]; 3];
        let lanes = self.l_v.iter().zip(&self.c_v).zip(&self.scale);
        for (lane, ((&l_v, &c_v), &scale)) in lanes.enumerate() {
            let column = OkhsvColumn {
                cos: self.cos,
                sin: self.sin,
                l_v,
                c_v,
                scale,
            };
            let [r, g, b] = column.rgb_at(val);
            rgb[0][lane] = r;
            rgb[1][lane] = g;
            rgb[2][lane] = b;
        }
        rgb
    }
}

#[derive(Clone, Copy, Debug)]
struct Cusp {
    l: f32,
    c: f32,
}

impl Cusp {
    fn find(cos: f32, sin: f32) -> Self {
        let s = max_saturation(cos, sin);
        let l = peak_scale(Oklab {
            l: 1.0,
            a: s * cos,
            b: s * sin,
        });
        Self { l, c: l * s }
    }

    fn slopes(self) -> CuspSlopes {
        CuspSlopes {
            s: self.c / self.l,
            t: self.c / (1.0 - self.l),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CuspSlopes {
    s: f32,
    t: f32,
}

/// Greatest `C/L` this hue direction reaches inside sRGB: a polynomial fit per cube
/// face, then Halley steps. Worst chroma error over 3600 hues: one step `3.2e-3`,
/// two `2.3e-5`, three `1.1e-11`, so three is exact in `f32`.
fn max_saturation(a: f32, b: f32) -> f32 {
    // The first channel to go negative picks the fit and the matrix row the Halley
    // step differentiates.
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

/// Cube-root scale bringing `lab`'s brightest linear channel to one, pinning the
/// cusp and curved top onto the gamut.
fn peak_scale(lab: Oklab) -> f32 {
    let rgb = lab.to_color();
    let peak = rgb.r.max(rgb.g).max(rgb.b);
    debug_assert!(peak > 0.0, "a hue slice always has a positive peak");
    (1.0 / peak).cbrt()
}

fn toe(x: f32) -> f32 {
    f32::midpoint(
        TOE_K3 * x - TOE_K1,
        ((TOE_K3 * x - TOE_K1) * (TOE_K3 * x - TOE_K1) + 4.0 * TOE_K2 * TOE_K3 * x).sqrt(),
    )
}

fn toe_inv(x: f32) -> f32 {
    (x * x + TOE_K1 * x) / (TOE_K3 * (x + TOE_K2))
}

#[cfg(test)]
mod tests;
