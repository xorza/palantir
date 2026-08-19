use crate::primitives::color::Color;
use crate::text::glyph_font::GlyphFont;
use crate::text::{FontFamily, FontWeight};
use crate::widgets::theme::text_style::TextStyle;

#[derive(Debug, ::serde::Deserialize)]
pub(super) struct UncheckedTextStyle {
    font_size_px: f32,
    color: Color,
    line_height_mult: f32,
    family: FontFamily,
    weight: FontWeight,
}

impl TryFrom<UncheckedTextStyle> for TextStyle {
    type Error = &'static str;

    fn try_from(style: UncheckedTextStyle) -> Result<Self, Self::Error> {
        if !GlyphFont::metrics_are_valid(
            style.font_size_px,
            style.font_size_px * style.line_height_mult,
        ) {
            return Err(GlyphFont::METRICS_ERROR);
        }
        Ok(Self {
            font_size_px: style.font_size_px,
            color: style.color,
            line_height_mult: style.line_height_mult,
            family: style.family,
            weight: style.weight,
        })
    }
}

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
