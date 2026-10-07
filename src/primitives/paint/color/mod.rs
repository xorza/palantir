//! Colour in the forms one value takes on its way to the GPU: straight-alpha
//! linear f32 for authoring and blending, four f16 lanes for lowered records,
//! and four sRGB-encoded bytes for hex codes, gradient stops, mesh vertices and
//! image texels.
//!
//! Naming: channels, then width. A bare `Rgba` is linear light (the crate's CPU
//! convention); `Srgba` is encoded. Every conversion lives here, so the two
//! quantize policies cannot drift.

pub(crate) mod srgba_u8;

pub(crate) mod color_coords;
pub(crate) mod color_model;
pub(crate) mod hsv;
pub(crate) mod okhsv;
pub(crate) mod oklab;
pub(crate) mod rgba_f16;
mod srgb_transfer;

use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
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
// One 8-bit sRGB step is narrowest near black at `1 / 255 / 12.92 ≈ 3.0e-4`
// linear; `1/4096 ≈ 2.4e-4` stays under it on every channel.
#[animate(settle_eps = 1.0 / 4096.0)]
/// An RGBA colour in straight-alpha linear RGB, the space every blend,
/// anti-aliasing step and tween operates in. The sRGB encode happens on the
/// GPU at swapchain write.
///
/// - [`Self::srgb`] / [`Self::srgba`] / [`Self::hex`] / [`Self::from_srgba`]
///   read their argument as sRGB-encoded (the numbers CSS and Figma show) and
///   linearise it. Use these for colours a human picked.
/// - [`Self::new`] takes already-linear values.
///
/// Writing an sRGB value straight into the fields renders too bright.
/// Components may exceed `1.0` for HDR-shaped tween outputs. `Hash` is exact
/// but for signed zeros; `FloatHash::hash_visual` is the tolerant one.
///
/// [`Animatable::lerp`](crate::widget::Animatable::lerp) blends per channel,
/// unclamped, which is correct for linear straight-alpha storage; alpha
/// travels with the colour, so keep your own opacity with [`Self::with_alpha`].
/// [`Interpolation::Oklab`](crate::Interpolation) blends perceptually, for
/// gradients.
#[must_use]
pub struct RgbaF32 {
    /// Red, linear, nominally 0..1.
    pub r: f32,
    /// Green, linear, nominally 0..1.
    pub g: f32,
    /// Blue, linear, nominally 0..1.
    pub b: f32,
    /// Alpha, 0..1, straight; the shader premultiplies.
    pub a: f32,
}

impl hash::Hash for RgbaF32 {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// A colour under both of the crate's float tolerances. [`Hash`](std::hash::Hash)
/// is the equality-compatible half; the visual half is what a content cache
/// keys on. They must not meet in one key, or a difference the eye cannot see
/// would split it on one field and not another.
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
    /// Fully transparent black; [`Self::is_noop`] is `true`.
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

    /// Alpha is non-positive, NaN, or within `EPS` of zero: paints nothing.
    #[inline]
    pub const fn is_noop(self) -> bool {
        // Alpha decides visibility; colour channels are screened for NaN only
        // (see `RgbaF16::is_noop`).
        domain::is_invisible(self.a) || self.has_nan()
    }

    /// True if any channel is NaN. `const`, so [`Self::is_noop`] and the
    /// [`NanCheck`] impl share it.
    ///
    /// [`NanCheck`]: crate::primitives::math::nan::NanCheck
    #[inline]
    pub(crate) const fn has_nan(self) -> bool {
        self.r.is_nan() || self.g.is_nan() || self.b.is_nan() || self.a.is_nan()
    }

    /// Linear channels and a straight alpha, stored as given, with no
    /// encoding. A colour a human picked arrives through [`Self::srgb`] or
    /// [`Self::hex`].
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `(r, g, b)` in 0..1 sRGB-encoded space, linearised on the way in.
    pub const fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::srgba(r, g, b, 1.0)
    }
    /// [`Self::srgb`] with an explicit straight alpha, which is not linearised.
    pub const fn srgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: srgb_transfer::decode(r as f64),
            g: srgb_transfer::decode(g as f64),
            b: srgb_transfer::decode(b as f64),
            a,
        }
    }

    /// The colour channels multiplied by alpha, the form colour blends
    /// interpolate in (see `primitives::paint::brush`). Still an `RgbaF32`;
    /// dividing alpha back out is the caller's.
    #[inline]
    pub(crate) const fn premultiplied(self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
        }
    }

    /// Replace the alpha channel, preserving RGB (storage is straight alpha).
    pub const fn with_alpha(self, a: f32) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    /// This colour times `tint`, channel by channel with alpha: the rule for
    /// mesh, image and stroke tints.
    #[inline]
    pub(crate) fn tinted(self, tint: Self) -> Self {
        Self {
            r: self.r * tint.r,
            g: self.g * tint.g,
            b: self.b * tint.b,
            a: self.a * tint.a,
        }
    }

    /// Decode sRGB-encoded bytes; alpha is `a / 255`. `const`, so a hex literal
    /// can be a constant. Exact: a compile-time table lookup, so
    /// [`Self::to_srgba_u8`] returns every byte.
    pub const fn from_srgba(bytes: SrgbaU8) -> Self {
        Self {
            r: srgb_transfer::DECODED_BYTES[bytes.r as usize],
            g: srgb_transfer::DECODED_BYTES[bytes.g as usize],
            b: srgb_transfer::DECODED_BYTES[bytes.b as usize],
            a: bytes.a as f32 / 255.0,
        }
    }

    /// Packed `0xRRGGBB` sRGB literal, opaque: `#3366CC` is `hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::from_srgba(SrgbaU8::hex(rgb))
    }

    /// Encode to sRGB 8-bit bytes. Inverts [`Self::from_srgba`] exactly.
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

/// Wire format: a CSS-style hex string, `#rrggbb`, or `#rrggbbaa` when alpha
/// is not fully opaque.
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

/// The hex forms the wire format reads, so `parse` and deserialize agree. Not
/// trimmed: the caller decides what whitespace means.
impl str::FromStr for RgbaF32 {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_hex(value)
    }
}

/// Parse `#rrggbb` / `#rrggbbaa` (the `#` optional) into an sRGB [`RgbaF32`].
/// Input is untrusted, so every rejection is an `Err`. Lengths select on
/// bytes and digits are decoded by hand: indexing the `str` would panic on a
/// char boundary for 6- or 8-byte non-ASCII input (`"日本"`), and
/// `u8::from_str_radix` would accept a leading `+`.
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

/// One hex digit's value, either case; anything else is a rejection.
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
