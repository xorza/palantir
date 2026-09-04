//! Colour in the forms one value takes on its way to the GPU: straight-alpha
//! linear f32 for authoring and blending, four f16 lanes for the lowered
//! records, four linear bytes for gradient stops and vertex colours, and
//! four sRGB-encoded bytes for what a hex code or an image texel means.
//!
//! One naming rule across all of them: the channels, then the width. A bare
//! `Rgba` is linear light, the crate's convention everywhere on the CPU;
//! `Srgba` is the encoded form. Every conversion between them is here, so
//! the two quantize policies cannot drift apart.

pub(crate) mod srgba_u8;

use crate::animation::animatable::Animatable;
use crate::primitives::approx::FloatHash;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::nan::NanCheck;
use crate::primitives::num;
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
/// - [`Self::srgb`] / [`Self::srgba`] / [`Self::hex`] / [`Self::from_srgba`]
///   read their argument as **sRGB-encoded** — the numbers CSS, Figma, and
///   Photoshop show you — and linearise it for you. This is what you want
///   for colours a human picked.
/// - [`Self::new`] takes values that are **already linear**: tween outputs,
///   physically-derived values, interop with another linear pipeline.
///   **already linear**: tween outputs, physically-derived values, interop
///   with another linear pipeline.
///
/// Writing an sRGB-encoded value straight into the fields skips the
/// linearisation and will render too bright. Components may exceed `1.0`
/// for HDR-shaped tween outputs. Hashing is approximate (`1e-4`).
pub struct RgbaF32 {
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

impl std::hash::Hash for RgbaF32 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// A colour under both of the crate's float tolerances.
///
/// [`Hash`](std::hash::Hash) above is the equality-compatible half, since
/// `RgbaF32` compares by exact float equality. The visual half is what a
/// *content* cache keys on, and the two must never meet inside one key:
/// a paint type canonicalizing its width visually and its colour exactly
/// would let a difference the eye cannot resolve split that key on one
/// field and not the other.
impl FloatHash for RgbaF32 {
    #[inline]
    fn hash_eq<H: std::hash::Hasher>(&self, state: &mut H) {
        self.r.hash_eq(state);
        self.g.hash_eq(state);
        self.b.hash_eq(state);
        self.a.hash_eq(state);
    }

    #[inline]
    fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        self.r.hash_visual(state);
        self.g.hash_visual(state);
        self.b.hash_visual(state);
        self.a.hash_visual(state);
    }
}

impl RgbaF32 {
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
        // for NaN only. See `RgbaF16::is_noop` for why a NaN in a
        // non-alpha lane has to count as invisible.
        approx::noop_f32(self.a) || self.has_nan()
    }

    /// True if any channel is NaN. `const`, so [`Self::is_noop`] can
    /// reuse it instead of repeating the channel walk; the [`NanCheck`]
    /// impl below delegates here for the same reason.
    ///
    /// [`NanCheck`]: crate::primitives::nan::NanCheck
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        self.r.is_nan() || self.g.is_nan() || self.b.is_nan() || self.a.is_nan()
    }

    /// The type's own representation: linear channels and a straight alpha,
    /// stored as given. For tween outputs, physically-derived values, and
    /// interop with another linear pipeline.
    ///
    /// The one constructor with no encoding in its name, because it is the
    /// one that does no encoding. A colour a human picked arrives through
    /// [`Self::srgb`] or [`Self::hex`] instead.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `(r, g, b)` in 0..1 **sRGB-encoded** space — the numbers CSS, Figma
    /// and Photoshop show — linearised on the way in so blending and SDF AA
    /// happen in linear light.
    pub const fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::srgba(r, g, b, 1.0)
    }
    /// [`Self::srgb`] with an explicit alpha. `a` is straight and is *not*
    /// linearised — alpha is already linear.
    pub const fn srgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a,
        }
    }

    /// Replace the alpha channel, preserve RGB. Storage is linear /
    /// straight-alpha (see `RgbaF32` docs), so this is a one-field swap —
    /// no premultiply rebalancing.
    pub const fn with_alpha(self, a: f32) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// Per-channel linear interpolation toward `other`: `t = 0` is `self`,
    /// `t = 1` is `other`. Storage is linear / straight-alpha (see the
    /// [`RgbaF32`] docs), so a straight component blend is the correct one —
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
    pub fn lerp(self, other: Self, t: f32) -> Self {
        <Self as Animatable>::lerp(self, other, t)
    }

    /// Decode sRGB-encoded bytes. Alpha is not gamma-encoded — straight
    /// `a / 255`. `const`, and the [`From`] impl delegates here, so a hex
    /// literal can be a constant.
    pub const fn from_srgba(bytes: SrgbaU8) -> Self {
        Self::srgba(
            bytes.r as f32 / 255.0,
            bytes.g as f32 / 255.0,
            bytes.b as f32 / 255.0,
            bytes.a as f32 / 255.0,
        )
    }

    /// Packed 24-bit `0xRRGGBB` sRGB literal, opaque. Matches CSS hex
    /// notation: `#3366CC` → `RgbaF32::hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::from_srgba(SrgbaU8::hex(rgb))
    }
    /// Packed 32-bit `0xRRGGBBAA` sRGB+alpha literal. CSS-order (alpha last).
    pub const fn hexa(rgba: u32) -> Self {
        Self::from_srgba(SrgbaU8::hexa(rgba))
    }

    /// Encode to **sRGB** 8-bit bytes via the cubic-Newton inverse
    /// (`linear_to_srgb`): what an image texel, a CSS hex string or a
    /// number shown to a person means. The linear quantize is
    /// `RgbaU8::from`, and the two return different types so one cannot be
    /// handed where the other is wanted. Lossy round trip, ≤ 1 LSB per
    /// channel.
    pub fn to_srgba_u8(self) -> SrgbaU8 {
        let q = |x: f32| num::unit_to_u8(linear_to_srgb(x));
        SrgbaU8 {
            r: q(self.r),
            g: q(self.g),
            b: q(self.b),
            a: num::unit_to_u8(self.a),
        }
    }
}

impl From<SrgbaU8> for RgbaF32 {
    #[inline]
    fn from(bytes: SrgbaU8) -> Self {
        Self::from_srgba(bytes)
    }
}

/// A 4-byte **linear**-u8 colour, for places where 8-bit linear precision
/// is enough and footprint matters: gradient stops and mesh vertices.
///
/// The `From<RgbaF32>` / `From<RgbaU8>` pair is a straight linear quantize
/// — **no sRGB encode**. The sRGB-encoded byte form is its own type,
/// [`SrgbaU8`], reached through [`RgbaF32::to_srgba_u8`]; [`Self::hex`] /
/// [`Self::hexa`] read a hex code and decode it to linear bytes, so every
/// value of this type is linear whichever way it was built.
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
pub struct RgbaU8 {
    /// Red, linear, 0..255.
    pub r: u8,
    /// Green, linear, 0..255.
    pub g: u8,
    /// Blue, linear, 0..255.
    pub b: u8,
    /// Alpha, 0..255, straight.
    pub a: u8,
}

impl std::hash::Hash for RgbaU8 {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl RgbaU8 {
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

    /// Opaque colour from **linear** bytes. For a CSS hex code use
    /// [`Self::hex`]; for bytes that stay sRGB-encoded, [`SrgbaU8`].
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
    /// The type's own representation: linear bytes, stored as given. Use
    /// [`Self::hex`] / [`Self::hexa`] when the bytes come from a CSS hex
    /// code, and this when they are already linear — test fixtures,
    /// atlas-bake maths.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
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
        let bytes = SrgbaU8::hexa(rgba);
        // Alpha is linear by convention, so it crosses as the byte it
        // arrived as rather than through the quantize.
        let rgb = Self::from_linear(RgbaF32::from_srgba(bytes));
        Self {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
            a: bytes.a,
        }
    }

    /// **Linear** quantize — straight `(channel * 255) as u8`, no sRGB
    /// encoding. Used by every linear-storage consumer: vertex colours,
    /// gradient stops baked into the linear LUT, and the hex
    /// constructors above. [`RgbaF32::to_srgba_u8`] is the sRGB-encoded
    /// path, and it returns [`SrgbaU8`] rather than this type.
    ///
    /// The body [`From<RgbaF32>`](Self) runs, `const` so a `const fn`
    /// constructor can reach it too.
    pub(crate) const fn from_linear(c: RgbaF32) -> Self {
        Self {
            r: num::unit_to_u8(c.r),
            g: num::unit_to_u8(c.g),
            b: num::unit_to_u8(c.b),
            a: num::unit_to_u8(c.a),
        }
    }

    /// True when alpha is zero — paints nothing visible.
    #[inline]
    pub const fn is_noop(self) -> bool {
        self.a == 0
    }
}

impl From<RgbaF32> for RgbaU8 {
    /// The **linear** quantize — straight `(channel * 255) as u8`, no
    /// sRGB encoding. [`RgbaF32::to_srgba_u8`] is the sRGB-encoded path.
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Self::from_linear(c)
    }
}

impl From<RgbaU8> for RgbaF32 {
    /// **Linear** un-quantize — straight `u8 / 255.0`, mirrors the
    /// `From<RgbaF32>` linear pack. No sRGB decoding; bytes that are
    /// sRGB-encoded are an [`SrgbaU8`], which decodes through the cubic
    /// `srgb_to_linear`.
    #[inline]
    fn from(s: RgbaU8) -> Self {
        RgbaF32 {
            r: s.r as f32 / 255.0,
            g: s.g as f32 / 255.0,
            b: s.b as f32 / 255.0,
            a: s.a as f32 / 255.0,
        }
    }
}

/// Linear-RGB colour packed as four f16 lanes in 8 B (align 2).
/// Same lane scheme as `Corners` — pack and unpack go through
/// `F16x4::from_lanes` and `F16x4::lanes`, one SIMD instruction on
/// targets with hardware f16 support and a scalar walk otherwise. f16
/// carries ~3 decimal digits and the full f32 range — well below
/// display quantization.
///
/// Use this for storage sites that want half the footprint of
/// `RgbaF32` (16 B) without `RgbaU8`'s cubic-Newton sRGB roundtrip.
/// Pod-compatible; Hash delegates to [`F16x4`] (one `u64` write).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RgbaF16(F16x4);

impl RgbaF16 {
    pub const TRANSPARENT: Self = Self(F16x4::ZERO);

    /// True when alpha is below `EPS` — paints nothing visible. Reuses
    /// [`F16x4::lane_is_noop`]'s bit-trick (mask sign, compare against
    /// the `EPS` pattern) so no f16→f32 conversion is needed.
    #[inline]
    pub const fn is_noop(self) -> bool {
        // A NaN in *any* lane, not just alpha: an opaque colour with a
        // NaN red channel reaches the shader and renders as
        // hardware-dependent garbage. Covering all four costs one
        // masked add, not three extra compares.
        self.0.lane_is_noop(3) || self.0.has_nan()
    }

    /// True when alpha is within `EPS` of 1.0 — paints with full
    /// coverage. Mirror of `is_noop` at the opposite end of the
    /// scale; same bit-trick, no f16→f32 conversion.
    ///
    /// On this tier alone, though all three colour types carry `is_noop`:
    /// occlusion pruning is what asks, it runs in the composer, and the
    /// composer sees lowered colour. Authoring colour is never asked
    /// whether it is opaque.
    #[inline]
    pub const fn is_opaque(self) -> bool {
        self.0.lane_is_opaque(3)
    }

    /// All four lanes unpacked to f32 at once. Single instruction on
    /// F16C/fp16 targets.
    #[inline]
    pub fn unpack(self) -> RgbaF32 {
        let [r, g, b, a] = self.0.lanes();
        RgbaF32 { r, g, b, a }
    }

    /// The 8 storage bytes as one `u64` — used by the record store's
    /// solid-fill payload packing where a `RgbaF16` rides in a `u64`
    /// slot alongside the gradient-hash alternative.
    #[inline]
    pub(crate) fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl From<RgbaF32> for RgbaF16 {
    /// Four-lane f32→f16 pack — single instruction on F16C/fp16
    /// targets, scalar fallback elsewhere.
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Self(F16x4::from_lanes([c.r, c.g, c.b, c.a]))
    }
}

impl From<RgbaF16> for RgbaF32 {
    #[inline]
    fn from(c: RgbaF16) -> Self {
        c.unpack()
    }
}

/// Direct linear-f16 → linear-u8 quantize. Delegates through `RgbaF32`
/// so it can't drift from the two-hop form; exists because the
/// composer converts per text run / mesh / image / curve every frame
/// and the double conversion read as two distinct steps at call sites.
impl From<RgbaF16> for RgbaU8 {
    #[inline]
    fn from(c: RgbaF16) -> Self {
        RgbaU8::from(RgbaF32::from(c))
    }
}

/// Wire format: a CSS-style hex string, `#rrggbb` or `#rrggbbaa`. The
/// 6-digit form is emitted whenever alpha is fully opaque.
impl Serialize for RgbaF32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let SrgbaU8 { r, g, b, a } = self.to_srgba_u8();
        let hex = if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for RgbaF32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        parse_hex(raw.trim()).map_err(D::Error::custom)
    }
}

/// Parse `#rrggbb` / `#rrggbbaa` (the `#` optional) into an sRGB
/// [`RgbaF32`]. Deserialization input is untrusted, so every rejection is
/// an `Err` — the length arms select on **bytes** and each digit is
/// decoded by hand, because indexing the `str` instead would panic on a
/// char boundary for any 6- or 8-*byte* non-ASCII input (`"日本"` is
/// exactly six bytes), and delegating to `u8::from_str_radix` would
/// accept its leading `+` sign as a hex digit position.
fn parse_hex(value: &str) -> Result<RgbaF32, &'static str> {
    let body = value.strip_prefix('#').unwrap_or(value).as_bytes();
    let parse_byte = |index: usize| -> Result<u8, &'static str> {
        Ok(hex_nibble(body[index])? << 4 | hex_nibble(body[index + 1])?)
    };
    match body.len() {
        6 => Ok(RgbaF32::from_srgba(SrgbaU8::rgb(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
        ))),
        8 => Ok(RgbaF32::from_srgba(SrgbaU8::new(
            parse_byte(0)?,
            parse_byte(2)?,
            parse_byte(4)?,
            parse_byte(6)?,
        ))),
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

impl NanCheck for RgbaF16 {
    #[inline]
    fn has_nan(&self) -> bool {
        self.0.has_nan()
    }
}

impl NanCheck for RgbaF32 {
    #[inline]
    fn has_nan(&self) -> bool {
        RgbaF32::has_nan(*self)
    }
}

#[cfg(test)]
mod tests;
