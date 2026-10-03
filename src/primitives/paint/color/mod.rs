//! Colour in the forms one value takes on its way to the GPU: straight-alpha
//! linear f32 for authoring and blending, four f16 lanes for the lowered
//! records and every draw lane, and four sRGB-encoded bytes for what a hex
//! code, a gradient stop, a mesh vertex or an image texel means.
//!
//! One naming rule across all of them: the channels, then the width. A bare
//! `Rgba` is linear light, the crate's convention everywhere on the CPU;
//! `Srgba` is the encoded form. Every conversion between them lives in this
//! module, so the two quantize policies cannot drift apart.

pub(crate) mod srgba_u8;

pub(crate) mod color_coords;
pub(crate) mod color_model;
pub(crate) mod hsv;
pub(crate) mod okhsv;
pub(crate) mod oklab;
pub(crate) mod rgba_f16;
mod srgb_transfer;

use crate::animation::animatable::Animatable;
use crate::primitives::math::approx;
use crate::primitives::math::approx::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::math::num;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::hash;
use std::str;

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
///
/// Writing an sRGB-encoded value straight into the fields skips the
/// linearisation and will render too bright. Components may exceed `1.0`
/// for HDR-shaped tween outputs. Hashing is approximate (`1e-4`).
#[must_use]
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

impl hash::Hash for RgbaF32 {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
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
    fn hash_eq<H: hash::Hasher>(&self, state: &mut H) {
        self.r.hash_eq(state);
        self.g.hash_eq(state);
        self.b.hash_eq(state);
        self.a.hash_eq(state);
    }

    #[inline]
    fn hash_visual<H: hash::Hasher>(&self, state: &mut H) {
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
        approx::paints_nothing(self.a) || self.has_nan()
    }

    /// True if any channel is NaN. `const`, so [`Self::is_noop`] can
    /// reuse it instead of repeating the channel walk; the [`NanCheck`]
    /// impl below delegates here for the same reason.
    ///
    /// [`NanCheck`]: crate::primitives::math::nan::NanCheck
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
            r: srgb_transfer::decode(r as f64),
            g: srgb_transfer::decode(g as f64),
            b: srgb_transfer::decode(b as f64),
            a,
        }
    }

    /// The colour channels multiplied by alpha — the form every colour
    /// blend in the crate interpolates in (see `primitives::paint::brush`). The
    /// result is still an `RgbaF32`, so dividing the alpha back out before
    /// it is read as straight is the caller's.
    #[inline]
    pub(crate) const fn premultiplied(self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
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

    /// This colour times `tint`, channel by channel with alpha — the rule
    /// a mesh, image or stroke tint applies. Both are straight-alpha, so
    /// the product is too.
    #[inline]
    pub(crate) fn tinted(self, tint: Self) -> Self {
        Self {
            r: self.r * tint.r,
            g: self.g * tint.g,
            b: self.b * tint.b,
            a: self.a * tint.a,
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
    ///
    /// Exact: each channel is the transfer function of `byte / 255`
    /// rounded to `f32` once — a lookup into a table built at compile
    /// time — so [`Self::to_srgba_u8`] gives every byte back.
    pub const fn from_srgba(bytes: SrgbaU8) -> Self {
        Self {
            r: srgb_transfer::DECODED_BYTES[bytes.r as usize],
            g: srgb_transfer::DECODED_BYTES[bytes.g as usize],
            b: srgb_transfer::DECODED_BYTES[bytes.b as usize],
            a: bytes.a as f32 / 255.0,
        }
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

    /// Encode to **sRGB** 8-bit bytes: what an image texel, a CSS hex
    /// string or a number shown to a person means. Inverts
    /// [`Self::from_srgba`] exactly for every byte.
    pub fn to_srgba_u8(self) -> SrgbaU8 {
        SrgbaU8 {
            r: srgb_transfer::encode_byte(self.r),
            g: srgb_transfer::encode_byte(self.g),
            b: srgb_transfer::encode_byte(self.b),
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

impl From<RgbaF16> for RgbaF32 {
    #[inline]
    fn from(c: RgbaF16) -> Self {
        c.unpack()
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

/// The hex forms the wire format reads, so `"#3266cc".parse()` and a
/// deserialized `"#3266cc"` cannot disagree. Not trimmed: the caller decides
/// what whitespace means.
impl str::FromStr for RgbaF32 {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_hex(value)
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

impl NanCheck for RgbaF32 {
    #[inline]
    fn has_nan(&self) -> bool {
        RgbaF32::has_nan(*self)
    }
}

#[cfg(test)]
mod tests;
