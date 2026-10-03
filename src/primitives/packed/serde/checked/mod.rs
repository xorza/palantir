//! Field validators for file-borne scalars, one per kind of value.
//!
//! A theme file is untrusted data, and every scalar in it reaches a sink
//! that asserts its contract — `Sizing::fixed`, `set_gap`,
//! `Duration::from_secs_f32`. Each numeric field a theme carries names one
//! of these in `#[serde(deserialize_with)]`, so a bad value is a
//! deserialization error where it is read, and the code that uses the
//! value can treat it as already valid. Each is a thin wrapper over the
//! [`domain`] predicate of its kind, and reports the kind's rule, so a
//! file and a call site cannot disagree about a value.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain::{self, vec2};
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

/// A [length](domain::length).
pub(crate) fn length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_length, domain::LENGTH_RULE)
}

/// A [gap](domain::gap).
pub(crate) fn gap<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_gap, domain::GAP_RULE)
}

/// A [positive](domain::positive) rate or divisor.
pub(crate) fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_positive, domain::POSITIVE_RULE)
}

/// A [fraction](domain::fraction). A file states one in range: a value
/// outside `0..=1` is refused rather than clamped, because the author of a
/// file can fix it.
pub(crate) fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_fraction, domain::FRACTION_RULE)
}

/// A signed [offset](domain::offset).
pub(crate) fn offset<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_offset, domain::OFFSET_RULE)
}

/// An [angle](domain::angle).
pub(crate) fn angle<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(deserializer, domain::is_angle, domain::ANGLE_RULE)
}

/// A 2-D extent: both axes are [lengths](vec2::length).
pub(crate) fn length2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(deserializer, vec2::is_length, domain::LENGTH_RULE)
}

/// A 2-D [offset](vec2::offset).
pub(crate) fn offset2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(deserializer, vec2::is_offset, domain::OFFSET_RULE)
}

/// Spacing every edge of which passes `valid`, or the first edge that
/// fails, reported under `rule`.
fn spacing<'de, D: Deserializer<'de>>(
    deserializer: D,
    valid: fn(f32) -> bool,
    rule: &str,
) -> Result<Spacing, D::Error> {
    let spacing = Spacing::deserialize(deserializer)?;
    match spacing.as_array().into_iter().find(|&edge| !valid(edge)) {
        Some(bad) => Err(D::Error::custom(format_args!("{rule}, got {bad}"))),
        None => Ok(spacing),
    }
}

/// A padding: every edge a [length](domain::length).
pub(crate) fn padding<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    spacing(deserializer, domain::is_length, domain::LENGTH_RULE)
}

/// A margin: every edge an [offset](domain::offset).
pub(crate) fn margin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Spacing, D::Error> {
    spacing(deserializer, domain::is_offset, domain::OFFSET_RULE)
}

/// Three points in some unit space — a polyline like a checkmark: every
/// point an [offset](vec2::offset). Three, because serde implements arrays
/// per length rather than for any `N`.
pub(crate) fn offset_points3<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[Vec2; 3], D::Error> {
    let points = <[Vec2; 3]>::deserialize(deserializer)?;
    match points.iter().find(|point| !vec2::is_offset(**point)) {
        Some(bad) => Err(D::Error::custom(format_args!(
            "{}, got {bad}",
            domain::OFFSET_RULE
        ))),
        None => Ok(points),
    }
}

#[cfg(test)]
mod tests;
