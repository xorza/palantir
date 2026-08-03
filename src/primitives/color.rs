use crate::primitives::nan::NanCheck;
use crate::primitives::{approx, half_simd::F16x4};
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

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
/// An RGBA colour in **straight-alpha linear RGB**, the space every blend,
/// anti-aliasing step, and tween in the crate operates in. The sRGB encode
/// happens on the GPU when writing the swapchain.
///
/// Which constructor you reach for decides whether your input gets
/// linearised:
///
/// - [`Self::rgb`] / [`Self::rgba`] / [`Self::hex`] / [`Self::rgb_u8`] read
///   their argument as **sRGB-perceptual** — the numbers CSS, Figma, and
///   Photoshop show you — and linearise it for you. This is what you want
///   for colours a human picked.
/// - [`Self::linear_rgb`] / [`Self::linear_rgba`] take values that are
///   **already linear**: tween outputs, physically-derived values, interop
///   with another linear pipeline.
///
/// Writing an sRGB-encoded value straight into the fields skips the
/// linearisation and will render too bright. Components may exceed `1.0`
/// for HDR-shaped tween outputs. Hashing is approximate (`1e-4`).
pub struct Color {
    /// Red, linear, nominally 0..1.
    pub r: f32,
    /// Green, linear, nominally 0..1.
    pub g: f32,
    /// Blue, linear, nominally 0..1.
    pub b: f32,
    /// Alpha, 0..1. **Straight**, not premultiplied — the shader does the
    /// premultiply on the way to the blend unit.
    pub a: f32,
}

impl std::hash::Hash for Color {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        approx::hash_f32(self.r, state);
        approx::hash_f32(self.g, state);
        approx::hash_f32(self.b, state);
        approx::hash_f32(self.a, state);
    }
}

impl Color {
    /// Fully transparent black. [`Self::is_noop`] is `true` for it, so it
    /// paints nothing at all.
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    /// Opaque white.
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    /// Opaque black.
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
    #[inline]
    pub const fn is_noop(self) -> bool {
        // Alpha decides visibility; the colour channels are screened
        // for NaN only. See `ColorF16::is_noop` for why a NaN in a
        // non-alpha lane has to count as invisible.
        approx::noop_f32(self.a) || self.r.is_nan() || self.g.is_nan() || self.b.is_nan()
    }

    /// `(r, g, b)` in 0..1 sRGB space (the default — matches CSS, Figma, Photoshop).
    /// Linearized internally so blending and SDF AA happen correctly in linear space.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }
    /// [`Self::rgb`] with an explicit alpha. `a` is straight and is *not*
    /// linearised — alpha is already linear.
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
    /// [`Self::linear_rgb`] with an explicit straight alpha.
    pub const fn linear_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Replace the alpha channel, preserve RGB. Storage is linear /
    /// straight-alpha (see `Color` docs), so this is a one-field swap —
    /// no premultiply rebalancing.
    pub const fn with_alpha(self, a: f32) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// Per-channel midpoint of two colors. The symmetric form of
    /// [`Self::lerp`] at `t = 0.5`.
    pub const fn midpoint(self, other: Self) -> Self {
        Self {
            r: (self.r + other.r) * 0.5,
            g: (self.g + other.g) * 0.5,
            b: (self.b + other.b) * 0.5,
            a: (self.a + other.a) * 0.5,
        }
    }

    /// Per-channel linear interpolation toward `other`: `t = 0` is `self`,
    /// `t = 1` is `other`. Storage is linear / straight-alpha (see the
    /// [`Color`] docs), so a straight component blend is the correct one —
    /// no gamma round-trip, no de-premultiply.
    ///
    /// **Alpha travels with the color.** A caller that wants to shift only the
    /// hue and keep its own opacity — a resting tint pulled toward the
    /// background, say, where a separate rule already owns alpha — follows up
    /// with [`Self::with_alpha`].
    ///
    /// `t` is not clamped, so overshooting past either end is available on
    /// purpose. Blending in a perceptual space instead is what
    /// [`Interp::Oklab`](crate::Interp) does for gradients.
    pub const fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
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

    /// Quantize this linear-RGB colour to **sRGB-encoded** 8-bit packed
    /// bytes via the cubic-Newton inverse (`linear_to_srgb`). The default
    /// `From<Color> for ColorU8` is a **linear** quantize (no cubic); call
    /// this when a consumer needs sRGB-perceptual bytes (e.g. quantizing a
    /// theme colour into an sRGB image tile). Lossy roundtrip ≤ 1 LSB per
    /// channel.
    pub fn to_srgb_u8(self) -> ColorU8 {
        let q = |x: f32| -> u8 { (linear_to_srgb(x).clamp(0.0, 1.0) * 255.0).round() as u8 };
        ColorU8 {
            r: q(self.r),
            g: q(self.g),
            b: q(self.b),
            a: (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }
}

/// 4-byte **linear-u8** colour (`r, g, b, a` each 0..=255). Storage
/// form for places where 8-bit linear precision is sufficient and cache
/// footprint matters — currently `Stop.color` (gradient stops). Not
/// used for fill / stroke / shape colour where animation lerps demand
/// `f32` linear-space precision.
///
/// Default `From<Color>` / `From<ColorU8>` are straight linear quantize
/// pairs (no sRGB encode). For the sRGB-encoded form (CSS-style hex
/// input), use [`Self::hex`] / [`Self::hexa`] explicitly.
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
/// A 4-byte **linear**-u8 colour, for places where 8-bit linear precision
/// is enough and footprint matters (currently gradient stops).
///
/// The default `From<Color>` / `From<ColorU8>` pair is a straight linear
/// quantize — **no sRGB encode**. For the sRGB-encoded byte form (what a
/// CSS hex string means) use [`Color::to_srgb_u8`], or construct with
/// [`Self::hex`] / [`Self::hexa`], which read their argument as sRGB.
pub struct ColorU8 {
    /// Red, linear, 0..255.
    pub r: u8,
    /// Green, linear, 0..255.
    pub g: u8,
    /// Blue, linear, 0..255.
    pub b: u8,
    /// Alpha, 0..255, straight.
    pub a: u8,
}

impl std::hash::Hash for ColorU8 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl ColorU8 {
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    /// Opaque white.
    pub const WHITE: Self = Self {
        r: 0xff,
        g: 0xff,
        b: 0xff,
        a: 0xff,
    };
    /// Opaque black.
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0xff,
    };

    /// Opaque colour from **linear** bytes. For sRGB-perceptual bytes use
    /// [`Self::hex`].
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

    /// Per-channel rounding average — the straight-alpha linear
    /// midpoint, quantized to within one 8-bit step. Used for polyline
    /// join-chrome colors.
    #[inline]
    pub const fn midpoint(self, other: Self) -> Self {
        const fn avg(a: u8, b: u8) -> u8 {
            (a as u16 + b as u16).div_ceil(2) as u8
        }
        Self {
            r: avg(self.r, other.r),
            g: avg(self.g, other.g),
            b: avg(self.b, other.b),
            a: avg(self.a, other.a),
        }
    }
    /// Raw-byte constructor — bytes go straight into the struct, no
    /// colour-space conversion. Interpret the result as **linear u8**
    /// (the convention `ColorU8` carries everywhere downstream). Use
    /// [`Self::hex`] / [`Self::hexa`] when your bytes come from
    /// CSS-style sRGB hex codes; use this when you already have
    /// linear bytes (e.g. test fixtures, atlas-bake math).
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    /// CSS-style `0xRRGGBB` opaque hex — interpreted as **sRGB-
    /// perceptual** and **decoded to linear** during construction, so
    /// the stored bytes match the linear-u8 atlas convention. A
    /// previous mid-tone like `0x22ccdd` (sRGB-perceptual) lands as
    /// the matching linear-u8 triplet, not as the verbatim bytes —
    /// otherwise a linear-format LUT would display it wildly too
    /// bright. Opaque shorthand for [`Self::hexa`].
    pub const fn hex(rgb: u32) -> Self {
        Self::hexa((rgb << 8) | 0xff)
    }
    /// CSS-style `0xRRGGBBAA` hex with alpha — RGB sRGB-decoded to
    /// linear like [`Self::hex`]; alpha is linear by convention
    /// (matches CSS), passed through as `a/255`.
    pub const fn hexa(rgba: u32) -> Self {
        let r = ((rgba >> 24) & 0xff) as u8;
        let g = ((rgba >> 16) & 0xff) as u8;
        let b = ((rgba >> 8) & 0xff) as u8;
        let a = (rgba & 0xff) as u8;
        let c = Color::rgb_u8(r, g, b);
        Self {
            r: (c.r * 255.0 + 0.5) as u8,
            g: (c.g * 255.0 + 0.5) as u8,
            b: (c.b * 255.0 + 0.5) as u8,
            a,
        }
    }

    /// True when alpha is zero — paints nothing visible.
    #[inline]
    pub const fn is_noop(self) -> bool {
        self.a == 0
    }
}

impl From<Color> for ColorU8 {
    /// **Linear** quantize — straight `(channel * 255) as u8`, no
    /// sRGB encoding. Used by every linear-storage consumer (vertex
    /// colours, gradient stops baked into the linear LUT, etc.). Call
    /// [`Color::to_srgb_u8`] for the sRGB-encoded path.
    #[inline]
    fn from(c: Color) -> Self {
        let q = |x: f32| -> u8 { (x.clamp(0.0, 1.0) * 255.0).round() as u8 };
        ColorU8 {
            r: q(c.r),
            g: q(c.g),
            b: q(c.b),
            a: q(c.a),
        }
    }
}

impl From<ColorU8> for Color {
    /// **Linear** un-quantize — straight `u8 / 255.0`, mirrors the
    /// `From<Color>` linear pack. No sRGB decoding; if the bytes
    /// were sRGB-encoded use `Color::rgba_u8` (which goes through
    /// the cubic `srgb_to_linear`).
    #[inline]
    fn from(s: ColorU8) -> Self {
        Color {
            r: s.r as f32 / 255.0,
            g: s.g as f32 / 255.0,
            b: s.b as f32 / 255.0,
            a: s.a as f32 / 255.0,
        }
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
/// `Color` (16 B) without `ColorU8`'s cubic-Newton sRGB roundtrip.
/// Pod-compatible; Hash delegates to [`F16x4`] (one `u64` write).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColorF16(F16x4);

impl ColorF16 {
    pub const TRANSPARENT: Self = Self(F16x4::ZERO);

    /// True when alpha is below `EPS` — paints nothing visible. Reuses
    /// the shared `noop_f16_bits` bit-trick (mask sign, compare against
    /// `EPS` bits) so no f16→f32 conversion is needed.
    #[inline]
    pub const fn is_noop(self) -> bool {
        use crate::primitives::approx::noop_f16_bits;
        // A NaN in *any* lane, not just alpha: an opaque colour with a
        // NaN red channel reaches the shader and renders as
        // hardware-dependent garbage. Covering all four costs one
        // masked add, not three extra compares.
        noop_f16_bits(self.0.0[3]) || self.0.has_nan()
    }

    /// True when alpha is within `EPS` of 1.0 — paints with full
    /// coverage. Mirror of `is_noop` at the opposite end of the
    /// scale; same bit-trick, no f16→f32 conversion. Used by the
    /// composer to flag solid-fill quads as occlusion-pruning
    /// candidates.
    #[inline]
    pub const fn is_opaque(self) -> bool {
        use crate::primitives::approx::opaque_f16_bits;
        opaque_f16_bits(self.0.0[3])
    }

    /// All four lanes unpacked to f32 at once via the batched f16→f32
    /// slice path. Single instruction on F16C/fp16 targets.
    #[inline]
    pub fn unpack(self) -> Color {
        let [r, g, b, a] = self.0.lanes();
        Color { r, g, b, a }
    }

    /// The 8 storage bytes as one `u64` — used by the record store's
    /// solid-fill payload packing where a `ColorF16` rides in a `u64`
    /// slot alongside the gradient-hash alternative.
    #[inline]
    pub(crate) fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl From<Color> for ColorF16 {
    /// Batched f32→f16 pack via the slice path — single instruction
    /// on F16C/fp16 targets, scalar fallback elsewhere.
    #[inline]
    fn from(c: Color) -> Self {
        Self(F16x4::from_lanes([c.r, c.g, c.b, c.a]))
    }
}

impl From<ColorF16> for Color {
    #[inline]
    fn from(c: ColorF16) -> Self {
        c.unpack()
    }
}

/// Direct linear-f16 → linear-u8 quantize. Delegates through `Color`
/// so it can't drift from the two-hop form; exists because the
/// composer converts per text run / mesh / image / curve every frame
/// and the double conversion read as two distinct steps at call sites.
impl From<ColorF16> for ColorU8 {
    #[inline]
    fn from(c: ColorF16) -> Self {
        ColorU8::from(Color::from(c))
    }
}

/// Wire format: a CSS-style hex string, `#rrggbb` or `#rrggbbaa`. The
/// 6-digit form is emitted whenever alpha is fully opaque.
impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let ColorU8 { r, g, b, a } = self.to_srgb_u8();
        let hex = if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        parse_hex(raw.trim()).map_err(D::Error::custom)
    }
}

/// Parse `#rrggbb` / `#rrggbbaa` (the `#` optional) into an sRGB
/// [`Color`]. Deserialization input is untrusted, so every rejection is
/// an `Err` — the length arms select on **bytes** and each digit is
/// decoded by hand, because indexing the `str` instead would panic on a
/// char boundary for any 6- or 8-*byte* non-ASCII input (`"日本"` is
/// exactly six bytes), and delegating to `u8::from_str_radix` would
/// accept its leading `+` sign as a hex digit position.
fn parse_hex(value: &str) -> Result<Color, &'static str> {
    let body = value.strip_prefix('#').unwrap_or(value).as_bytes();
    let parse_byte = |index: usize| -> Result<u8, &'static str> {
        Ok(hex_nibble(body[index])? << 4 | hex_nibble(body[index + 1])?)
    };
    match body.len() {
        6 => Ok(Color::rgb_u8(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
        )),
        8 => Ok(Color::rgba_u8(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
            parse_byte(6)?,
        )),
        _ => Err("expected #rrggbb or #rrggbbaa"),
    }
}

/// One hex digit's value, either case. Anything else — including every
/// non-ASCII byte — is a rejection.
const fn hex_nibble(byte: u8) -> Result<u8, &'static str> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("invalid hex digit"),
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

impl NanCheck for ColorF16 {
    #[inline]
    fn has_nan(&self) -> bool {
        self.0.has_nan()
    }
}

impl NanCheck for Color {
    #[inline]
    fn has_nan(&self) -> bool {
        self.r.is_nan() || self.g.is_nan() || self.b.is_nan() || self.a.is_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_value(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

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

    #[test]
    fn color_parse_accepts_with_and_without_hash() {
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
        // Either digit case, both lengths. The serializer only ever
        // emits lowercase, so nothing else pins that a hand-written
        // uppercase theme file still parses.
        assert_eq!(
            parse_hex("#3266CC").unwrap(),
            Color::rgb_u8(0x32, 0x66, 0xcc)
        );
        assert_eq!(
            parse_hex("#3266CC80").unwrap(),
            Color::rgba_u8(0x32, 0x66, 0xcc, 0x80)
        );
    }

    #[test]
    fn color_parse_rejects_malformed_input() {
        assert!(parse_hex("").is_err());
        assert!(parse_hex("#").is_err());
        assert!(parse_hex("#abc").is_err(), "3-digit not supported");
        assert!(parse_hex("#abcde").is_err(), "5-digit not supported");
        assert!(parse_hex("#abcdefab12").is_err(), "10-digit too long");
        assert!(parse_hex("#zzzzzz").is_err(), "non-hex digits");
        // Regression: the length arms select on bytes, so these reach a
        // digit decode rather than a `str` index. `"日本"` is 6 bytes and
        // `"αβγδ"` is 8, hitting both arms; indexing the `str` split a
        // char boundary and panicked instead of rejecting the input —
        // in a deserializer whose whole job is to reject it.
        assert!(parse_hex("日本").is_err(), "6-byte non-ASCII");
        assert!(parse_hex("#日本").is_err(), "6-byte non-ASCII, hashed");
        assert!(parse_hex("αβγδ").is_err(), "8-byte non-ASCII");
        // `u8::from_str_radix` accepts a leading sign, so the old parser
        // read `"+a+b+c"` as rgb(10, 11, 12).
        assert!(parse_hex("#+a+b+c").is_err(), "sign is not a hex digit");
    }

    #[test]
    fn equal_signed_zero_colors_have_equal_hashes() {
        let positive = Color::linear_rgba(0.0, 0.0, 0.0, 0.0);
        let negative = Color::linear_rgba(-0.0, -0.0, -0.0, -0.0);

        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));
    }

    #[test]
    fn lerp_spans_both_endpoints_and_agrees_with_midpoint() {
        // Every channel here is an exact binary fraction, so the arithmetic
        // below is checkable by eye and by `==`.
        let a = Color::linear_rgba(1.0, 0.0, 0.5, 0.5);
        let b = Color::linear_rgba(0.0, 1.0, 0.5, 0.0);

        // The endpoints come back exactly, alpha included.
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);

        // Hand-computed quarter step: r 1.0→0.75, g 0.0→0.25, b flat at 0.5,
        // a 0.5→0.375. Alpha travels with the color, unlike `with_alpha`.
        let quarter = a.lerp(b, 0.25);
        assert_eq!(
            (quarter.r, quarter.g, quarter.b, quarter.a),
            (0.75, 0.25, 0.5, 0.375)
        );

        // `midpoint` is the symmetric spelling of the same blend.
        let mid = a.lerp(b, 0.5);
        let sym = a.midpoint(b);
        for (l, r) in [
            (mid.r, sym.r),
            (mid.g, sym.g),
            (mid.b, sym.b),
            (mid.a, sym.a),
        ] {
            assert!((l - r).abs() < 1e-6, "lerp(0.5) = {l}, midpoint = {r}");
        }

        // `t` is deliberately unclamped, so a caller can overshoot: t = 2
        // continues past `b` by the same delta again (r 1.0 → -1.0).
        assert_eq!(a.lerp(b, 2.0).r, -1.0);
    }

    /// The direct `ColorF16 → ColorU8` quantize must stay byte-identical
    /// to the two-hop form (`ColorU8::from(Color::from(x))`) it replaced
    /// at the composer's per-run/tint call sites.
    #[test]
    fn f16_to_u8_matches_two_hop_quantize() {
        for c in [
            Color::linear_rgba(0.0, 0.0, 0.0, 0.0),
            Color::linear_rgba(1.0, 0.25, 0.5, 1.0),
            Color::linear_rgba(0.1, 0.9, 0.33, 0.5),
            Color::linear_rgba(1.0, 1.0, 1.0, 1.0),
        ] {
            let f16 = ColorF16::from(c);
            assert_eq!(ColorU8::from(f16), ColorU8::from(Color::from(f16)));
        }
    }
}
