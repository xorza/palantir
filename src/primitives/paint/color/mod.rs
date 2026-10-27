//! Color in the forms one value takes on its way to the GPU: straight-alpha
//! linear f32 for authoring and blending, four f16 lanes for lowered records,
//! and four sRGB-encoded bytes for hex codes, gradient stops, mesh vertices and
//! image texels.
//!
//! Naming: channels, then width. A bare `Rgba` is linear light (the crate's CPU
//! convention); `Srgba` is encoded. Every conversion lives here, so the two
//! quantize policies cannot drift.

pub(crate) mod color_coords;
pub(crate) mod color_model;
pub(crate) mod hsv;
pub(crate) mod okhsv;
pub(crate) mod oklab;
pub(crate) mod rgba_f16;
mod srgb_transfer;
pub(crate) mod srgba_u8;

use crate::primitives::math::animatable::Animatable;
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
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
/// An RGBA color in straight-alpha linear RGB, the space every blend,
/// anti-aliasing step and tween operates in. The sRGB encode happens on the
/// GPU at swapchain write.
///
/// - [`Self::srgb`] / [`Self::srgba`] / [`Self::hex`] / [`Self::from_srgba`]
///   read their argument as sRGB-encoded (the numbers CSS and Figma show) and
///   linearize it. Use these for colors a human picked.
/// - [`Self::new`] takes already-linear values.
///
/// Writing an sRGB value straight into the fields renders too bright.
/// Components may exceed `1.0` for HDR-shaped tween outputs. `Hash` is exact
/// but for signed zeros; `FloatHash::hash_visual` is the tolerant one.
///
/// [`Animatable::lerp`] and springs blend premultiplied
/// (CSS Color 4 §12.3), unclamped: a fade to [`Self::TRANSPARENT`] keeps its hue instead of
/// passing through a darker one. Alpha travels with the color, so keep your own opacity with
/// [`Self::with_alpha`].
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

/// One 8-bit sRGB step is narrowest near black at `1 / 255 / 12.92 ≈ 3.0e-4` linear; `1/4096 ≈
/// 2.4e-4` stays under it on every channel.
const SETTLE_EPS: f32 = 1.0 / 4096.0;

/// Motion space is premultiplied, so a spring blends as a tween does; the arithmetic is per
/// channel there, alpha one more channel.
impl Animatable for RgbaF32 {
    /// The ends are exact: `t` of 0 or 1 returns `a` or `b` with no premultiply round trip.
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        a.premultiplied()
            .mix(b.premultiplied(), t)
            .unpremultiplied()
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self::new(
            self.r - other.r,
            self.g - other.g,
            self.b - other.b,
            self.a - other.a,
        )
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self::new(
            self.r + other.r,
            self.g + other.g,
            self.b + other.b,
            self.a + other.a,
        )
    }

    #[inline]
    fn scale(self, k: f32) -> Self {
        Self::new(self.r * k, self.g * k, self.b * k, self.a * k)
    }

    #[inline]
    fn magnitude_squared(self) -> f32 {
        self.r * self.r + self.g * self.g + self.b * self.b + self.a * self.a
    }

    #[inline]
    fn settle_distance_squared(self) -> f32 {
        self.magnitude_squared() / (SETTLE_EPS * SETTLE_EPS)
    }

    #[inline]
    fn zero() -> Self {
        Self::TRANSPARENT
    }

    #[inline]
    fn to_motion(self) -> Self {
        self.premultiplied()
    }

    #[inline]
    fn from_motion(motion: Self) -> Self {
        motion.unpremultiplied()
    }
}

impl hash::Hash for RgbaF32 {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.hash_eq(state);
    }
}

/// A color under both of the crate's float tolerances. [`Hash`](std::hash::Hash)
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
        // Alpha decides visibility; color channels are screened for NaN only
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
    /// encoding. A color a human picked arrives through [`Self::srgb`] or
    /// [`Self::hex`].
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `(r, g, b)` in 0..1 sRGB-encoded space, linearized on the way in.
    pub const fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::srgba(r, g, b, 1.0)
    }
    /// [`Self::srgb`] with an explicit straight alpha, which is not linearized.
    pub const fn srgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: srgb_transfer::decode(r as f64),
            g: srgb_transfer::decode(g as f64),
            b: srgb_transfer::decode(b as f64),
            a,
        }
    }

    /// The color channels multiplied by alpha, the form color blends
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

    /// Straight alpha from premultiplied: the inverse of [`Self::premultiplied`]. With no alpha
    /// there is no color to recover, so the channels are zero.
    #[inline]
    pub(crate) fn unpremultiplied(self) -> Self {
        if self.a > 0.0 {
            Self::new(self.r / self.a, self.g / self.a, self.b / self.a, self.a)
        } else {
            Self::new(0.0, 0.0, 0.0, self.a)
        }
    }

    /// Per channel from `self` toward `other` at `t`, alpha as one more channel: the blend of two
    /// premultiplied colors.
    #[inline]
    pub(crate) const fn mix(self, other: Self, t: f32) -> Self {
        Self::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }

    /// [`Self::with_alpha`] in 255ths, the step the hex form holds, so a default color is saved as
    /// hex and loads back the same.
    pub(crate) const fn with_alpha8(self, a: u8) -> Self {
        self.with_alpha(alpha_of_byte(a))
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

    /// This color times `tint`, channel by channel with alpha: the rule for
    /// mesh, image and stroke tints.
    #[inline]
    pub(crate) const fn tinted(self, tint: Self) -> Self {
        Self {
            r: self.r * tint.r,
            g: self.g * tint.g,
            b: self.b * tint.b,
            a: self.a * tint.a,
        }
    }

    /// Decode sRGB-encoded bytes; alpha is `a / 255`. `const`, so a hex literal
    /// can be a constant. Exact: a compile-time table lookup, so
    /// [`Self::to_srgba`] returns every byte.
    pub const fn from_srgba(bytes: SrgbaU8) -> Self {
        Self {
            r: srgb_transfer::DECODED_BYTES[bytes.r as usize],
            g: srgb_transfer::DECODED_BYTES[bytes.g as usize],
            b: srgb_transfer::DECODED_BYTES[bytes.b as usize],
            a: alpha_of_byte(bytes.a),
        }
    }

    /// Packed `0xRRGGBB` sRGB literal, opaque: `#3366CC` is `hex(0x3366CC)`.
    ///
    /// # Panics
    ///
    /// As [`SrgbaU8::hex`].
    #[track_caller]
    pub const fn hex(rgb: u32) -> Self {
        Self::from_srgba(SrgbaU8::hex(rgb))
    }

    /// Packed `0xRRGGBBAA` sRGB literal, alpha last as CSS orders it: [`SrgbaU8::hexa`],
    /// decoded.
    pub const fn hexa(rgba: u32) -> Self {
        Self::from_srgba(SrgbaU8::hexa(rgba))
    }

    /// Pack to four f16 lanes, rounding each channel to f16: what a draw lane carries.
    #[inline]
    pub(crate) fn to_f16(self) -> RgbaF16 {
        RgbaF16::new(self.r, self.g, self.b, self.a)
    }

    /// Encode to sRGB 8-bit bytes. Inverts [`Self::from_srgba`] exactly.
    pub fn to_srgba(self) -> SrgbaU8 {
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

/// Wire format: CSS hex, `#rrggbb` or `#rrggbbaa` when alpha is not fully opaque, for a color on
/// the 8-bit sRGB grid; otherwise CSS Color 4's `color(srgb-linear r g b / a)`, the stored linear
/// channels in shortest round-trip form, with `/ a` left out for an opaque one. Either way the
/// color loads back the same. Refused: hex alone, which moves every color off the grid.
impl Serialize for RgbaF32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bytes = self.to_srgba();
        let SrgbaU8 { r, g, b, a } = bytes;
        let text = if Self::from_srgba(bytes) != *self {
            let Self { r, g, b, a } = *self;
            if a == 1.0 {
                format!("color(srgb-linear {r} {g} {b})")
            } else {
                format!("color(srgb-linear {r} {g} {b} / {a})")
            }
        } else if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        serializer.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for RgbaF32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        parse(raw.trim()).map_err(D::Error::custom)
    }
}

/// The forms the wire format reads, so `parse` and deserialize agree. Not
/// trimmed: the caller decides what whitespace means.
impl str::FromStr for RgbaF32 {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse(value)
    }
}

/// An 8-bit alpha as a straight alpha: the one conversion, so a color built from 255ths is on the
/// grid [`RgbaF32::from_srgba`] reads.
const fn alpha_of_byte(a: u8) -> f32 {
    a as f32 / 255.0
}

/// A hex color, or `color(srgb-linear …)`.
fn parse(value: &str) -> Result<RgbaF32, &'static str> {
    match value.strip_prefix("color(") {
        Some(body) => parse_linear(body),
        None => parse_hex(value),
    }
}

/// Parse the body of `color(srgb-linear r g b)` or `color(srgb-linear r g b / a)` after its
/// `color(`: plain numbers, as [`Serialize`] writes them, every channel finite. CSS's
/// percentages and `none` are not read, nor any space but `srgb-linear`.
fn parse_linear(body: &str) -> Result<RgbaF32, &'static str> {
    const FORM: &str = "expected color(srgb-linear r g b) or color(srgb-linear r g b / a)";
    let inner = body
        .strip_suffix(')')
        .and_then(|inner| inner.trim_start().strip_prefix("srgb-linear"))
        .filter(|channels| channels.starts_with(char::is_whitespace))
        .ok_or(FORM)?;
    let (channels, alpha) = match inner.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha)),
        None => (inner, None),
    };
    let numbers = |text: &str, count: usize| -> Result<Vec<f32>, &'static str> {
        let values = text
            .split_whitespace()
            .map(|word| word.parse::<f32>().map_err(|_| FORM))
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() == count {
            Ok(values)
        } else {
            Err(FORM)
        }
    };
    let rgb = numbers(channels, 3)?;
    let a = match alpha {
        Some(alpha) => numbers(alpha, 1)?[0],
        None => 1.0,
    };
    let color = RgbaF32::new(rgb[0], rgb[1], rgb[2], a);
    if domain::is_color(color) {
        Ok(color)
    } else {
        Err(domain::COLOR_RULE)
    }
}

/// Parse `#rrggbb` / `#rrggbbaa` (the `#` optional) into an sRGB [`RgbaF32`].
/// Input is untrusted, so every rejection is an `Err`. Lengths select on
/// bytes and digits are decoded by hand: indexing the `str` would panic on a
/// char boundary for 6- or 8-byte non-ASCII input (`"日本"`), and
/// `u8::from_str_radix` would accept a leading `+`.
fn parse_hex(value: &str) -> Result<RgbaF32, &'static str> {
    let body = value.strip_prefix('#').unwrap_or(value).as_bytes();
    let parse_byte = |idx: usize| -> Result<u8, &'static str> {
        Ok((hex_nibble(body[idx])? << 4) | hex_nibble(body[idx + 1])?)
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
        Self::has_nan(*self)
    }
}

#[cfg(test)]
mod tests;
