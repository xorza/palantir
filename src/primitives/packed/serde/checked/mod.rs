//! Field validators for file-borne scalars. A theme file is untrusted and its scalars reach asserting sinks (`Sizing::fixed`, `set_gap`, `Duration::from_secs_f32`); each numeric field names one of these in `#[serde(deserialize_with)]`, so a bad value is a deserialization error. Each wraps the [`domain`] predicate of its kind, so file and call site agree; a length or an offset is also held to [`domain::MAX_FILE_LENGTH`], which the packed stores and their sums need, and a gap is a length here.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::paint::backdrop_filter::Lens;
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer};
use glam::Vec2;
use std::fmt::Display;

fn read<'de, D: Deserializer<'de>, T: Deserialize<'de> + Display + Copy>(
    deserializer: D,
    valid: impl Fn(T) -> bool,
    rule: &str,
) -> Result<T, D::Error> {
    let value = T::deserialize(deserializer)?;
    if valid(value) {
        Ok(value)
    } else {
        Err(D::Error::custom(format_args!("{rule}, got {value}")))
    }
}

/// A [length](domain::length) of at most [`domain::MAX_FILE_LENGTH`], which also serves a
/// [gap](domain::gap).
pub(crate) fn length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        domain::is_file_length,
        domain::FILE_LENGTH_RULE,
    )
}

/// A [positive](domain::positive) rate or divisor.
pub(crate) fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_positive, domain::POSITIVE_RULE)
}

/// A [factor](domain::factor).
pub(crate) fn factor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_factor, domain::FACTOR_RULE)
}

/// A [refractive index](crate::Lens::is_refractive_index): finite and at least 1.
pub(crate) fn refractive_index<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<f32, D::Error> {
    read(
        deserializer,
        Lens::is_refractive_index,
        Lens::REFRACTIVE_INDEX_RULE,
    )
}

/// A [fraction](domain::fraction). A file's value outside `0..=1` is refused, not clamped.
pub(crate) fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_fraction, domain::FRACTION_RULE)
}

/// A signed [offset](domain::offset) within ±[`domain::MAX_FILE_LENGTH`].
pub(crate) fn offset<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        domain::is_file_offset,
        domain::FILE_OFFSET_RULE,
    )
}

/// An [angle](domain::angle).
pub(crate) fn angle<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_angle, domain::ANGLE_RULE)
}

/// A 2-D extent: both axes are [file lengths](vec2::is_file_length).
pub(crate) fn length2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(deserializer, vec2::is_file_length, domain::FILE_LENGTH_RULE)
}

/// A 2-D offset: both axes are [file offsets](vec2::is_file_offset).
pub(crate) fn offset2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(deserializer, vec2::is_file_offset, domain::FILE_OFFSET_RULE)
}

/// Packed lanes that all pass `valid`, or the first failing lane, reported under `rule`.
fn lanes<'de, D: Deserializer<'de>, T: Deserialize<'de> + LaneCodec>(
    deserializer: D,
    valid: fn(f32) -> bool,
    rule: &str,
) -> Result<T, D::Error> {
    let value = T::deserialize(deserializer)?;
    match value.to_lane_array().into_iter().find(|&lane| !valid(lane)) {
        Some(bad) => Err(D::Error::custom(format_args!("{rule}, got {bad}"))),
        None => Ok(value),
    }
}

/// A padding: every edge a [file length](domain::is_file_length).
pub(crate) fn padding<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    lanes(
        deserializer,
        domain::is_file_length,
        domain::FILE_LENGTH_RULE,
    )
}

/// A margin: every edge a [file offset](domain::is_file_offset).
pub(crate) fn margin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    lanes(
        deserializer,
        domain::is_file_offset,
        domain::FILE_OFFSET_RULE,
    )
}

/// Corner radii: every one a [file length](domain::is_file_length).
pub(crate) fn corners<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Corners, D::Error> {
    lanes(
        deserializer,
        domain::is_file_length,
        domain::FILE_LENGTH_RULE,
    )
}

/// Three points (a polyline like a checkmark), each a [file offset](vec2::is_file_offset). Three because serde implements arrays per length.
pub(crate) fn offset_points3<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[Vec2; 3], D::Error> {
    let points = <[Vec2; 3]>::deserialize(deserializer)?;
    match points.iter().find(|point| !vec2::is_file_offset(**point)) {
        Some(bad) => Err(D::Error::custom(format_args!(
            "{}, got {bad}",
            domain::FILE_OFFSET_RULE
        ))),
        None => Ok(points),
    }
}

#[cfg(test)]
mod tests;
