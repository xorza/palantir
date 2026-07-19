use ::serde::de::Error as _;

use crate::primitives::color::Color;
use crate::text::{FontFamily, FontWeight, TEXT_METRICS_ERROR, TextMetrics};
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::{TEXT_SCALE_ERROR, text_scale_is_valid};

#[derive(Debug, ::serde::Deserialize)]
pub(crate) struct UncheckedTextStyle {
    font_size_px: f32,
    color: Color,
    line_height_mult: f32,
    family: FontFamily,
    weight: FontWeight,
}

impl TryFrom<UncheckedTextStyle> for TextStyle {
    type Error = &'static str;

    fn try_from(style: UncheckedTextStyle) -> Result<Self, Self::Error> {
        TextMetrics::from_size_and_multiplier(style.font_size_px, style.line_height_mult)
            .map_err(|_| TEXT_METRICS_ERROR)?;
        Ok(Self {
            font_size_px: style.font_size_px,
            color: style.color,
            line_height_mult: style.line_height_mult,
            family: style.family,
            weight: style.weight,
        })
    }
}

pub(crate) fn deserialize_text_scale<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: ::serde::Deserializer<'de>,
{
    let scale = <f32 as ::serde::Deserialize>::deserialize(deserializer)?;
    if !text_scale_is_valid(scale) {
        return Err(D::Error::custom(TEXT_SCALE_ERROR));
    }
    Ok(scale)
}

pub(crate) mod duration_seconds {
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
