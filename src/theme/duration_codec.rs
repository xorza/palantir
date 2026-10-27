//! Duration codecs for theme files: a `#[serde(with = ...)]` codec applies to a field, so it has no
//! type to sit beside. A file writes seconds as an `f64`, whose 53-bit mantissa holds any duration
//! a theme sets to the nanosecond, as `f32` does not (300 ms read back as 300 000 012 ns).

/// Any duration, zero included.
pub(super) mod seconds {
    use ::serde::de::Error as _;
    use ::serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    const DURATION_RULE: &str = "a duration must be finite, not negative, and representable";

    pub(crate) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(duration.as_secs_f64())
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(deserializer)?;
        Duration::try_from_secs_f64(secs)
            .map_err(|error| D::Error::custom(format_args!("{DURATION_RULE}: {error}")))
    }
}

/// The length of one cycle: above zero, as a zero period never moves. A value that rounds to
/// zero nanoseconds is refused too.
pub(super) mod period {
    use ::serde::de::Error as _;
    use ::serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    const PERIOD_RULE: &str = "a period must be a duration above zero";

    pub(crate) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        super::seconds::serialize(duration, serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(deserializer)?;
        match Duration::try_from_secs_f64(secs) {
            Ok(period) if !period.is_zero() => Ok(period),
            _ => Err(D::Error::custom(format_args!("{PERIOD_RULE}, got {secs}"))),
        }
    }
}
