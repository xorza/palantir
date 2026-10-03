//! Field validators for file-borne scalars, one per kind of value.
//!
//! A theme file is untrusted data, and every scalar in it reaches a sink
//! that asserts its contract — `Sizing::fixed`, `set_gap`,
//! `Duration::from_secs_f32`. Each numeric field a theme carries names one
//! of these in `#[serde(deserialize_with)]`, so a bad value is a
//! deserialization error where it is read, and the code that uses the
//! value can treat it as already valid. The field types stay `f32`: a
//! value built in code is the caller's own contract, checked where it is
//! used.

use crate::layout::types::limits::MAX_PACKED_GAP;
use ::serde::de::Error as _;
use ::serde::{Deserialize, Deserializer};
use glam::Vec2;
use std::fmt::Display;

fn read<'de, D: Deserializer<'de>, T: Deserialize<'de> + Display>(
    deserializer: D,
    valid: impl Fn(&T) -> bool,
    rule: &str,
) -> Result<T, D::Error> {
    let value = T::deserialize(deserializer)?;
    if valid(&value) {
        Ok(value)
    } else {
        Err(D::Error::custom(format_args!("{rule}, got {value}")))
    }
}

/// A distance: finite and not negative.
pub(crate) fn length<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        |v: &f32| v.is_finite() && *v >= 0.0,
        "a theme length must be finite and not negative",
    )
}

/// A container gap: a length that fits the packed f16 gap lane.
pub(crate) fn gap<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        |v: &f32| v.is_finite() && (0.0..=MAX_PACKED_GAP).contains(v),
        "a theme gap must be finite, not negative, and at most 65504",
    )
}

/// A rate or a divisor: finite and above zero.
pub(crate) fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        |v: &f32| v.is_finite() && *v > 0.0,
        "a theme rate must be finite and above zero",
    )
}

/// A share of something: in `0.0..=1.0`.
pub(crate) fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        |v: &f32| (0.0..=1.0).contains(v),
        "a theme fraction must be in 0..=1",
    )
}

/// An angle or a signed offset: any finite value.
pub(crate) fn finite<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    read(
        deserializer,
        |v: &f32| v.is_finite(),
        "a theme value must be finite",
    )
}

/// A 2-D extent: both axes are lengths.
pub(crate) fn length2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(
        deserializer,
        |v: &Vec2| v.is_finite() && v.x >= 0.0 && v.y >= 0.0,
        "a theme extent must be finite and not negative",
    )
}

/// A 2-D offset: both axes finite, either sign.
pub(crate) fn finite2<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec2, D::Error> {
    read(
        deserializer,
        |v: &Vec2| v.is_finite(),
        "a theme offset must be finite",
    )
}

/// Three points in some unit space — a polyline like a checkmark:
/// every coordinate finite. Three, because serde implements arrays
/// per length rather than for any `N`.
pub(crate) fn finite_points3<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[Vec2; 3], D::Error> {
    let points = <[Vec2; 3]>::deserialize(deserializer)?;
    match points.iter().find(|point| !point.is_finite()) {
        Some(bad) => Err(D::Error::custom(format_args!(
            "a theme point must be finite, got {bad}"
        ))),
        None => Ok(points),
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::serde::checked;
    use glam::Vec2;

    /// Run one validator over `text` read as RON.
    fn check<T>(
        validator: fn(&mut ron::Deserializer<'static>) -> Result<T, ron::Error>,
        text: &'static str,
    ) -> Result<T, String> {
        let mut deserializer = ron::Deserializer::from_str(text).expect("RON");
        validator(&mut deserializer).map_err(|error| error.to_string())
    }

    /// Each scalar rule at its ends: the last value it takes and the
    /// first it refuses, and the non-finite values every rule refuses.
    /// 65504 is the largest f16; 65505 packs to infinity.
    #[test]
    fn each_scalar_rule_holds_its_ends() {
        type Rule = fn(&mut ron::Deserializer<'static>) -> Result<f32, ron::Error>;
        let rows: [(&str, Rule, &[&'static str], &[&'static str]); 5] = [
            (
                "length",
                |d| checked::length(d),
                &["0.0", "1e30"],
                &["-1.0", "inf", "NaN"],
            ),
            (
                "gap",
                |d| checked::gap(d),
                &["0.0", "65504.0"],
                &["-1.0", "65505.0", "NaN"],
            ),
            (
                "positive",
                |d| checked::positive(d),
                &["1e-30"],
                &["0.0", "-1.0", "inf"],
            ),
            (
                "fraction",
                |d| checked::fraction(d),
                &["0.0", "1.0"],
                &["-0.5", "1.5", "NaN"],
            ),
            (
                "finite",
                |d| checked::finite(d),
                &["-1e30", "1e30"],
                &["inf", "-inf", "NaN"],
            ),
        ];
        for (name, rule, takes, refuses) in rows {
            for &text in takes {
                assert!(check(rule, text).is_ok(), "{name} takes {text}");
            }
            for &text in refuses {
                let error = check(rule, text).expect_err(text);
                assert!(error.contains("a theme "), "{name} on {text}: {error}");
            }
        }
    }

    /// The vector rules check every axis: one bad axis refuses the value.
    #[test]
    fn vector_rules_check_every_axis() {
        assert_eq!(
            check(|d| checked::length2(d), "(1.0, 2.0)"),
            Ok(Vec2::new(1.0, 2.0))
        );
        assert!(check(|d| checked::length2(d), "(1.0, -2.0)").is_err());
        assert_eq!(
            check(|d| checked::finite2(d), "(-1.0, 2.0)"),
            Ok(Vec2::new(-1.0, 2.0))
        );
        assert!(check(|d| checked::finite2(d), "(inf, 2.0)").is_err());
        assert!(
            check(
                |d| checked::finite_points3(d),
                "((0.0, 0.0), (1.0, 1.0), (2.0, 0.0))"
            )
            .is_ok()
        );
        let error = check(
            |d| checked::finite_points3(d),
            "((0.0, 0.0), (NaN, 1.0), (2.0, 0.0))",
        )
        .expect_err("a NaN point");
        assert!(error.contains("a theme point must be finite"), "{error}");
    }
}
