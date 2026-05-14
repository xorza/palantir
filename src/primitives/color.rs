use half::f16;

#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Default,
    bytemuck::Pod,
    bytemuck::Zeroable,
    palantir_anim_derive::Animatable,
)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl std::hash::Hash for Color {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

// Serialize as a CSS-style sRGB hex string: `"#RRGGBB"` when fully
// opaque, `"#RRGGBBAA"` otherwise. Round-trips through the same `u8`
// quantization the framework uses everywhere (1/255 is below 8-bit
// display precision, well below the cubic sRGB approximation error).
impl serde::Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let r = (linear_to_srgb(self.r).clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (linear_to_srgb(self.g).clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (linear_to_srgb(self.b).clamp(0.0, 1.0) * 255.0).round() as u8;
        let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        s.serialize_str(&hex)
    }
}

impl<'de> serde::Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        parse_hex(raw.trim()).map_err(serde::de::Error::custom)
    }
}

/// Parse `#rrggbb` / `#rrggbbaa` (CSS-style, alpha last).
fn parse_hex(s: &str) -> Result<Color, &'static str> {
    let body = s.strip_prefix('#').unwrap_or(s);
    let parse_byte = |i: usize| -> Result<u8, &'static str> {
        u8::from_str_radix(&body[i..i + 2], 16).map_err(|_| "invalid hex digit")
    };
    match body.len() {
        6 => {
            let r = parse_byte(0)?;
            let g = parse_byte(2)?;
            let b = parse_byte(4)?;
            Ok(Color::rgb_u8(r, g, b))
        }
        8 => {
            let r = parse_byte(0)?;
            let g = parse_byte(2)?;
            let b = parse_byte(4)?;
            let a = parse_byte(6)?;
            Ok(Color::rgba_u8(r, g, b, a))
        }
        _ => Err("expected #rrggbb or #rrggbbaa"),
    }
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    /// Alpha is non-positive, NaN, or within `EPS` of zero —
    /// paints nothing. Mirrors the `is_noop` predicate on `Stroke`
    /// / `Background` / `Surface` / `ShapeRecord`; consistent name
    /// across primitives.
    pub const fn is_noop(self) -> bool {
        super::approx::noop_f32(self.a)
    }

    /// `(r, g, b)` in 0..1 sRGB space (the default — matches CSS, Figma, Photoshop).
    /// Linearized internally so blending and SDF AA happen correctly in linear space.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a,
        }
    }

    /// `(r, g, b)` already in linear (scene-referred) space. Use for tween outputs,
    /// physically-derived values, or interop with linear pipelines.
    pub const fn linear_rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
    pub const fn linear_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Multiply the linear RGB channels by `mul`, preserve alpha. Used by the
    /// encoder to dim disabled subtrees.
    pub const fn dim_rgb(self, mul: f32) -> Self {
        Self {
            r: self.r * mul,
            g: self.g * mul,
            b: self.b * mul,
            a: self.a,
        }
    }

    /// Scale all four channels by `mul`. Premultiplied-correct fade:
    /// scaling RGB and A by the same factor moves a premultiplied
    /// color along the line toward transparent. Stroke tessellation
    /// uses this for the hairline (<1 phys px) alpha fade.
    pub const fn scale_premultiplied(self, mul: f32) -> Self {
        Self {
            r: self.r * mul,
            g: self.g * mul,
            b: self.b * mul,
            a: self.a * mul,
        }
    }

    /// Per-channel midpoint of two colors.
    pub const fn midpoint(self, other: Self) -> Self {
        Self {
            r: (self.r + other.r) * 0.5,
            g: (self.g + other.g) * 0.5,
            b: (self.b + other.b) * 0.5,
            a: (self.a + other.a) * 0.5,
        }
    }

    /// 8-bit sRGB channels (Figma/CSS/Photoshop convention). Linearized
    /// internally, same as `Color::rgb`. `#3366CC` → `Color::rgb_u8(0x33, 0x66, 0xCC)`.
    pub const fn rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
    /// `rgb_u8` with 8-bit alpha. Alpha is not gamma-encoded — straight `a / 255`.
    pub const fn rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    /// Packed 24-bit `0xRRGGBB` sRGB literal, opaque. Matches CSS hex
    /// notation: `#3366CC` → `Color::hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb_u8(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }
    /// Packed 32-bit `0xRRGGBBAA` sRGB+alpha literal. CSS-order (alpha last).
    pub const fn hexa(rgba: u32) -> Self {
        Self::rgba_u8(
            ((rgba >> 24) & 0xff) as u8,
            ((rgba >> 16) & 0xff) as u8,
            ((rgba >> 8) & 0xff) as u8,
            (rgba & 0xff) as u8,
        )
    }

    /// Quantize this linear-RGB colour to 8-bit sRGB packed bytes via
    /// the cubic-Newton inverse (`linear_to_srgb`). Lossy roundtrip is
    /// bounded by 1 LSB per channel, matching the byte serializer's
    /// roundtrip pin.
    pub fn to_srgb8(self) -> Srgb8 {
        let q = |x: f32| -> u8 { (linear_to_srgb(x).clamp(0.0, 1.0) * 255.0).round() as u8 };
        Srgb8 {
            r: q(self.r),
            g: q(self.g),
            b: q(self.b),
            a: (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }
}

/// 4-byte sRGB-packed colour (`r, g, b, a` each 0..=255). Storage form
/// for places where 8-bit display precision is sufficient and cache
/// footprint matters — currently `Stop.color` (gradient stops). Not
/// used for fill / stroke / shape colour where animation lerps demand
/// `f32` linear-space precision.
///
/// Convert to/from `Color` via `From` impls (cubic Hejl-Burgess-Dawson
/// pair from this module). Roundtrip is bounded by 1 LSB per channel
/// (pinned by `color.rs` byte-roundtrip tests).
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Srgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl std::hash::Hash for Srgb8 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl Srgb8 {
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const WHITE: Self = Self {
        r: 0xff,
        g: 0xff,
        b: 0xff,
        a: 0xff,
    };
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0xff,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Pack the four channels into a single `u32` as `0xRRGGBBAA`
    /// (big-endian byte order — R in the most-significant byte). Used
    /// by hash sites that want to write one `u32`/`u64` instead of
    /// four `u8`s, cutting hasher dispatch and per-byte mixing.
    #[inline]
    pub const fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    /// `0xRRGGBB` opaque.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }
    /// `0xRRGGBBAA` with alpha.
    pub const fn hexa(rgba: u32) -> Self {
        Self::rgba(
            ((rgba >> 24) & 0xff) as u8,
            ((rgba >> 16) & 0xff) as u8,
            ((rgba >> 8) & 0xff) as u8,
            (rgba & 0xff) as u8,
        )
    }

    /// True when alpha is zero — paints nothing visible.
    pub const fn is_noop(self) -> bool {
        self.a == 0
    }
}

impl From<Color> for Srgb8 {
    #[inline]
    fn from(c: Color) -> Self {
        c.to_srgb8()
    }
}

impl From<Srgb8> for Color {
    #[inline]
    fn from(s: Srgb8) -> Self {
        Color::rgba_u8(s.r, s.g, s.b, s.a)
    }
}

/// Linear-RGB colour packed as four f16 lanes in 8 B (align 2).
/// Same lane scheme as `Corners` — pack/unpack go through
/// `half::slice::HalfFloatSliceExt::{convert_from_f32_slice,
/// convert_to_f32_slice}`, which map to one SIMD instruction on
/// targets with hardware f16 support (`fcvtn`/`fcvtl` on
/// aarch64-fp16, `vcvtps2ph`/`vcvtph2ps` on x86-f16c) and fall back
/// to scalar otherwise. f16 carries ~3 decimal digits and the full
/// f32 range — well below display quantization.
///
/// Use this for storage sites that want half the footprint of
/// `Color` (16 B) without `Srgb8`'s cubic-Newton sRGB roundtrip.
/// Pod-compatible; the hash impl writes the whole struct as one u64.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColorF16(pub [u16; 4]);

impl ColorF16 {
    pub const TRANSPARENT: Self = Self([0, 0, 0, 0]);

    /// True when alpha is below `EPS` — paints nothing visible. Reuses
    /// the shared `noop_f16_bits` bit-trick (mask sign, compare against
    /// `EPS` bits) so no f16→f32 conversion is needed.
    #[inline]
    pub fn is_noop(self) -> bool {
        use crate::primitives::approx::noop_f16_bits;
        noop_f16_bits(self.0[3])
    }

    /// All four lanes unpacked to f32 at once via the batched f16→f32
    /// slice path. Single instruction on F16C/fp16 targets.
    #[inline]
    pub fn unpack(self) -> Color {
        use half::slice::HalfFloatSliceExt;
        let arr: &[f16; 4] = bytemuck::cast_ref(&self.0);
        let mut out = [0.0f32; 4];
        arr.as_slice().convert_to_f32_slice(&mut out);
        Color {
            r: out[0],
            g: out[1],
            b: out[2],
            a: out[3],
        }
    }
}

impl From<Color> for ColorF16 {
    /// Batched f32→f16 pack via the slice path — single instruction
    /// on F16C/fp16 targets, scalar fallback elsewhere.
    #[inline]
    fn from(c: Color) -> Self {
        use half::slice::HalfFloatSliceExt;
        let src = [c.r, c.g, c.b, c.a];
        let mut out = [f16::ZERO; 4];
        out.as_mut_slice().convert_from_f32_slice(&src);
        Self(bytemuck::cast(out))
    }
}

impl From<ColorF16> for Color {
    #[inline]
    fn from(c: ColorF16) -> Self {
        c.unpack()
    }
}

impl std::hash::Hash for ColorF16 {
    /// Hash the 8 storage bytes as one `u64` — single hasher call
    /// instead of four `write_u16`s. Mirrors `Corners::hash`.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(u64::from_ne_bytes(bytemuck::cast(self.0)));
    }
}

/// sRGB→linear via cubic polynomial. Const-friendly (`f32::powf` is not
/// const-stable; see rust-lang/rust#57241). Industry-standard cubic fit
/// (Hejl-Burgess-Dawson and similar) over `[0, 1]`; max abs error ~1.5e-3
/// in linear space — well below 8-bit display precision (1/255 ≈ 4e-3),
/// so the difference is invisible in rendered output. Pinned by
/// `tests::cubic_srgb_max_error_under_two_thousandths`.
const fn srgb_to_linear(c: f32) -> f32 {
    c * (c * (c * 0.305_306_01 + 0.682_171_1) + 0.012_522_878)
}

// Used by the gradient LUT bake; no other in-crate caller until slice 2
// wires the atlas through the encoder/composer. `dead_code` would
// otherwise trip `clippy -D warnings` on a clean step-1-only branch.
#[allow(dead_code)]
/// Linear-RGB → Oklab. Matrix constants from Björn Ottosson's reference
/// (https://bottosson.github.io/posts/oklab/). Used by the gradient LUT
/// bake when `Interp::Oklab` is selected — interpolation in Oklab gives
/// perceptually-uniform transitions without the muddy red↔green
/// midpoint that linear-RGB lerps produce. Output components are
/// roughly `L ∈ 0..1, a/b ∈ -0.5..0.5`.
#[inline]
pub(crate) fn linear_to_oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    [
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    ]
}

#[allow(dead_code)]
/// Inverse of `linear_to_oklab`. Cube of the intermediate trichromatic
/// values can be negative for out-of-gamut Oklab values — gradient
/// lerps stay in-gamut by construction (both endpoints are valid
/// linear sRGB), so this is fine for the bake path.
#[inline]
pub(crate) fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = lab[0];
    let a = lab[1];
    let b = lab[2];
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3,
        -0.004_196_086_4 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

/// Inverse of the cubic `srgb_to_linear`. Used by the serde
/// serializer so that `serialize → parse → re-serialize` round-trips
/// to the exact same hex bytes (a spec-exact piecewise inverse would
/// drift by 1 LSB at certain values because it doesn't match the
/// cubic's curve). Spec-exact piecewise gives a great Newton seed —
/// 3 iterations converge to f32 precision over `[0, 1]`.
fn linear_to_srgb(y: f32) -> f32 {
    let mut x = if y <= 0.003_130_8 {
        y * 12.92
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    };
    for _ in 0..3 {
        let f = srgb_to_linear(x) - y;
        let f_prime = 3.0 * 0.305_306_01 * x * x + 2.0 * 0.682_171_1 * x + 0.012_522_878;
        x -= f / f_prime;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference: spec-exact piecewise sRGB→linear (the previous in-tree
    /// implementation). Used as ground truth for the cubic approximation.
    fn srgb_to_linear_exact(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Pin: the cubic stays within ~1.5e-3 of the spec-exact piecewise
    /// curve across `[0, 1]`. A regression past 2e-3 suggests the
    /// coefficients drifted; revisit before shipping.
    #[test]
    fn cubic_srgb_max_error_under_two_thousandths() {
        let mut max_err: f32 = 0.0;
        // Sweep at 1/1024 resolution — finer than 8-bit display, plenty
        // to catch the worst-case point.
        for i in 0..=1024 {
            let c = i as f32 / 1024.0;
            let approx = srgb_to_linear(c);
            let exact = srgb_to_linear_exact(c);
            let err = (approx - exact).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(
            max_err < 2.0e-3,
            "cubic max error {max_err} exceeded 2e-3 threshold"
        );
    }

    /// Sanity: const-construction works in const context. If `Color::rgb`
    /// regresses to non-const, this fails to compile.
    #[test]
    fn rgb_is_const_constructible() {
        const _LITERAL: Color = Color::rgb(0.2, 0.4, 0.8);
        const _HEX: Color = Color::hex(0x3366CC);
    }

    /// Roundtrip a Color through TOML and parse the emitted hex back.
    /// Wraps in a tiny struct because TOML's top level must be a table.
    fn toml_roundtrip(c: Color) -> (String, Color) {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct W {
            c: Color,
        }
        let s = toml::to_string(&W { c }).expect("serialize");
        let parsed: W = toml::from_str(&s).expect("parse");
        (s, parsed.c)
    }

    /// Pin: serializing a Color and re-serializing the parse converges
    /// to the same hex bytes for every (r, g, b) sRGB byte. Catches
    /// Newton-iteration regressions that drift by 1 LSB.
    #[test]
    fn hex_round_trip_stable_over_all_bytes() {
        for byte in 0u8..=255 {
            let c = Color::rgb_u8(byte, byte, byte);
            let (s1, parsed) = toml_roundtrip(c);
            let (s2, _) = toml_roundtrip(parsed);
            assert_eq!(s1, s2, "byte {byte} did not round-trip stably");
        }
    }

    /// Pin: alpha = 1.0 emits the 6-digit form; any other alpha emits
    /// the 8-digit form. A refactor that always emits 8 digits would
    /// silently change the output format and trip this test.
    #[test]
    fn opaque_emits_six_digits_translucent_emits_eight() {
        // 0.2 → 0x33, 0.4 → 0x66, 0.8 → 0xcc once round-tripped through
        // the cubic / Newton inverse pair.
        let (s, _) = toml_roundtrip(Color::rgb(0.2, 0.4, 0.8));
        assert!(
            s.contains(r##""#3366cc""##),
            "opaque must emit 6 digits: {s}"
        );
        let (s, _) = toml_roundtrip(Color::rgba(0.2, 0.4, 0.8, 0.5));
        assert!(
            s.contains(r##""#3366cc80""##),
            "translucent must emit 8 digits: {s}"
        );
    }

    /// Edge cases: fully transparent, fully opaque white, opaque black.
    #[test]
    fn extremes_round_trip() {
        for c in [Color::TRANSPARENT, Color::WHITE, Color::BLACK] {
            let (s1, p) = toml_roundtrip(c);
            let (s2, _) = toml_roundtrip(p);
            assert_eq!(s1, s2);
        }
    }

    /// Pin parse acceptance: with or without `#`, both 6- and 8-digit.
    #[test]
    fn parse_accepts_with_and_without_hash() {
        assert_eq!(
            parse_hex("#3266cc").unwrap(),
            Color::rgb_u8(0x32, 0x66, 0xcc)
        );
        assert_eq!(
            parse_hex("3266cc").unwrap(),
            Color::rgb_u8(0x32, 0x66, 0xcc)
        );
        assert_eq!(
            parse_hex("#3266cc80").unwrap(),
            Color::rgba_u8(0x32, 0x66, 0xcc, 0x80)
        );
    }

    /// Pin parse rejection: malformed inputs return an error rather
    /// than silently producing garbage.
    #[test]
    fn parse_rejects_malformed_input() {
        assert!(parse_hex("").is_err());
        assert!(parse_hex("#").is_err());
        assert!(parse_hex("#abc").is_err(), "3-digit not supported");
        assert!(parse_hex("#abcde").is_err(), "5-digit not supported");
        assert!(parse_hex("#abcdefab12").is_err(), "10-digit too long");
        assert!(parse_hex("#zzzzzz").is_err(), "non-hex digits");
    }
}
