//! Wire-format policy no single type owns: a `#[serde(with = ...)]` codec applies to a field, so it has no type to sit beside.

pub(super) mod duration_seconds {
    use std::time::Duration;

    use ::serde::de::Error as _;

    const ERROR: &str = "tooltip timing must be finite, non-negative, and representable";

    pub(crate) fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        serializer.serialize_f32(duration.as_secs_f32())
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        let secs = <f32 as ::serde::Deserialize>::deserialize(deserializer)?;
        Duration::try_from_secs_f32(secs).map_err(|_| D::Error::custom(ERROR))
    }
}
