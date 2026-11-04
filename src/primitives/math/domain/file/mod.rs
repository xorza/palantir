//! Field validators for file-borne scalars: the [`domain`] kinds applied to serde. A theme file
//! is untrusted and its scalars reach asserting sinks (`Sizing::fixed`, `set_gap`,
//! `Duration::from_secs_f32`); each numeric field names one of these in
//! `#[serde(deserialize_with)]`, so a bad value is a deserialization error. Each wraps the
//! [`domain`] predicate of its kind, so file and call site agree; a length or an offset is also
//! held to [`domain::MAX_FILE_LENGTH`], which the packed stores and their sums need, and a gap is a
//! length here.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::packed::serde::LaneCodec;
use ::serde::{Deserialize, Deserializer, de};
use glam::Vec2;
use std::fmt::Display;

/// One rule a value must keep, and its text for the error.
#[derive(Clone, Copy, Debug)]
struct Rule<T> {
    valid: fn(T) -> bool,
    text: &'static str,
}

const FILE_LENGTH: Rule<f32> = Rule {
    valid: domain::is_file_length,
    text: domain::FILE_LENGTH_RULE,
};
const FILE_OFFSET: Rule<f32> = Rule {
    valid: domain::is_file_offset,
    text: domain::FILE_OFFSET_RULE,
};
const FONT_LENGTH: Rule<f32> = Rule {
    valid: domain::is_font_length,
    text: domain::FONT_LENGTH_RULE,
};
const LANE: Rule<f32> = Rule {
    valid: domain::is_lane,
    text: domain::LANE_RULE,
};
const FACTOR: Rule<f32> = Rule {
    valid: domain::is_factor,
    text: domain::FACTOR_RULE,
};

/// The first of `rules` `value` breaks, as a deserialization error.
fn check<T: Display + Copy, E: de::Error>(value: T, rules: &[Rule<T>]) -> Result<T, E> {
    match rules.iter().find(|rule| !(rule.valid)(value)) {
        None => Ok(value),
        Some(rule) => Err(E::custom(format_args!("{}, got {value}", rule.text))),
    }
}

/// A `T` that keeps every one of `rules`, the one reader every scalar validator is.
fn read<'de, D: Deserializer<'de>, T: Deserialize<'de> + Display + Copy>(
    deserializer: D,
    rules: &[Rule<T>],
) -> Result<T, D::Error> {
    check(T::deserialize(deserializer)?, rules)
}

/// [`read`] for a field a file may leave out.
fn read_optional<'de, D: Deserializer<'de>, T: Deserialize<'de> + Display + Copy>(
    deserializer: D,
    rules: &[Rule<T>],
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)?
        .map(|value| check(value, rules))
        .transpose()
}

/// Packed lanes that all keep `rule`, or the first failing lane.
fn lanes<'de, D: Deserializer<'de>, T: Deserialize<'de> + LaneCodec>(
    deserializer: D,
    rule: Rule<f32>,
) -> Result<T, D::Error> {
    let value = T::deserialize(deserializer)?;
    for lane in value.to_lane_array() {
        check::<f32, D::Error>(lane, &[rule])?;
    }
    Ok(value)
}

/// A [length](domain::length) of at most [`domain::MAX_FILE_LENGTH`], which also serves a
/// [gap](domain::gap).
pub(crate) fn length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, &[FILE_LENGTH])
}

/// A font size: one the shaper accepts, above the UI epsilon, and a length a file may hold.
pub(crate) fn font_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, &[FONT_LENGTH, FILE_LENGTH])
}

/// An optional [`font_size`].
pub(crate) fn optional_font_size<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f32>, D::Error> {
    read_optional(deserializer, &[FONT_LENGTH, FILE_LENGTH])
}

/// A [positive](domain::positive) rate or divisor.
pub(crate) fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: domain::is_positive,
            text: domain::POSITIVE_RULE,
        }],
    )
}

/// A [length](domain::length) a file may hold that packs into an f16
/// [lane](domain::lane), as a backdrop filter's blur does.
pub(crate) fn lane_length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, &[FILE_LENGTH, LANE])
}

/// A [factor](domain::factor) that packs into an f16 [lane](domain::lane), as a backdrop
/// filter's saturation and grain do.
pub(crate) fn lane_factor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, &[FACTOR, LANE])
}

/// An optional [`lane_factor`], as a backdrop filter's luminance bounds.
pub(crate) fn optional_lane_factor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f32>, D::Error> {
    read_optional(deserializer, &[FACTOR, LANE])
}

/// A [refractive index](domain::refractive_index): finite and at least 1.
pub(crate) fn refractive_index<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<f32, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: domain::is_refractive_index,
            text: domain::REFRACTIVE_INDEX_RULE,
        }],
    )
}

/// A [fraction](domain::fraction). A file's value outside `0..=1` is refused, not clamped.
pub(crate) fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: domain::is_fraction,
            text: domain::FRACTION_RULE,
        }],
    )
}

/// A signed [offset](domain::offset) within ±[`domain::MAX_FILE_LENGTH`].
pub(crate) fn offset<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, &[FILE_OFFSET])
}

/// An [angle](domain::angle).
pub(crate) fn angle<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: domain::is_angle,
            text: domain::ANGLE_RULE,
        }],
    )
}

/// A 2-D extent: both axes are [file lengths](domain::is_file_length), so a file names both, an
/// axis left out reading as unbounded.
pub(crate) fn size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Size, D::Error> {
    let size = Size::deserialize(deserializer)?;
    check::<f32, D::Error>(size.w, &[FILE_LENGTH])?;
    check::<f32, D::Error>(size.h, &[FILE_LENGTH])?;
    Ok(size)
}

/// An ellipse's two radii: both axes are [file lengths](vec2::is_file_length).
pub(crate) fn length2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: vec2::is_file_length,
            text: domain::FILE_LENGTH_RULE,
        }],
    )
}

/// A 2-D offset: both axes are [file offsets](vec2::is_file_offset).
pub(crate) fn offset2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(
        deserializer,
        &[Rule {
            valid: vec2::is_file_offset,
            text: domain::FILE_OFFSET_RULE,
        }],
    )
}

/// A padding: every edge a [file length](domain::is_file_length).
pub(crate) fn padding<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    lanes(deserializer, FILE_LENGTH)
}

/// A margin: every edge a [file offset](domain::is_file_offset).
pub(crate) fn margin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    lanes(deserializer, FILE_OFFSET)
}

/// Corner radii: every one a [file length](domain::is_file_length).
pub(crate) fn corners<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Corners, D::Error> {
    lanes(deserializer, FILE_LENGTH)
}

/// Three points (a polyline like a checkmark), each a [file offset](vec2::is_file_offset). Three
/// because serde implements arrays per length.
pub(crate) fn offset_points3<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[Vec2; 3], D::Error> {
    let points = <[Vec2; 3]>::deserialize(deserializer)?;
    let rule = Rule {
        valid: vec2::is_file_offset,
        text: domain::FILE_OFFSET_RULE,
    };
    for point in points {
        check::<Vec2, D::Error>(point, &[rule])?;
    }
    Ok(points)
}

#[cfg(test)]
mod tests;
