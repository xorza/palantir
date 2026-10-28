//! Why text cannot make an [`RgbaF32`](crate::RgbaF32).

use crate::primitives::math::domain;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why [`RgbaF32`](crate::RgbaF32)'s `FromStr` refused its text; the text comes from theme files
/// and typed input, so each flaw is an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseColorError {
    /// A hex color whose text after the optional `#` is not six or eight bytes long.
    Length,
    /// A hex color with a character that is not a hex digit.
    Digit,
    /// A `color(` form that is not `color(srgb-linear r g b)` or `color(srgb-linear r g b / a)`.
    LinearForm,
    /// A `color(srgb-linear …)` channel that is not finite or is above 65504, the largest f16.
    Channel,
}

impl Display for ParseColorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "expected #rrggbb or #rrggbbaa",
            Self::Digit => "invalid hex digit",
            Self::LinearForm => "expected color(srgb-linear r g b) or color(srgb-linear r g b / a)",
            Self::Channel => domain::COLOR_RULE,
        })
    }
}

impl error::Error for ParseColorError {}
