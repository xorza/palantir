//! Per-axis overrides a text widget or a theme slot folds onto a resolved
//! [`TextStyle`].

use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::text_style::TextStyle;
use serde::de::Error as _;

/// Per-axis overrides folded onto a resolved [`TextStyle`].
///
/// What a text widget collects from its one-axis setters
/// ([`Text::color`](crate::Text::color),
/// [`TextEdit::font_size`](crate::TextEdit::font_size)), so a caller wanting
/// one axis need not build a whole style. A `None` field leaves the resolved
/// value standing. Every text slot in a theme past
/// [`Theme::text`](crate::Theme) is one, so a disabled look that dims the ink
/// keeps following its size and face. A `None` axis is omitted from theme
/// files.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
#[must_use]
pub struct TextStyleOverrides {
    /// Replaces [`TextStyle::color`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<RgbaF32>,
    /// Replaces [`TextStyle::font_size`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Replaces [`TextStyle::line_height_factor`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height_factor: Option<f32>,
    /// Replaces [`TextStyle::family`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<FontFamily>,
    /// Replaces [`TextStyle::weight`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<FontWeight>,
    /// Replaces [`TextStyle::slant`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slant: Option<FontSlant>,
}

impl TextStyleOverrides {
    /// Overrides nothing.
    pub const NONE: Self = Self {
        color: None,
        font_size: None,
        line_height_factor: None,
        family: None,
        weight: None,
        slant: None,
    };

    /// `base` with every axis this set names replaced.
    #[inline]
    pub fn apply(self, base: &TextStyle) -> TextStyle {
        TextStyle {
            font_size: self.font_size.unwrap_or(base.font_size),
            color: self.color.unwrap_or(base.color),
            line_height_factor: self.line_height_factor.unwrap_or(base.line_height_factor),
            family: self.family.unwrap_or(base.family),
            weight: self.weight.unwrap_or(base.weight),
            slant: self.slant.unwrap_or(base.slant),
        }
    }

    /// Give `self` each axis only `other` names, from `ambient`, and `other` each axis only `self`
    /// names, from `other_ambient`: the values each shows over its own ambient style, so neither
    /// shows anything new.
    pub(crate) fn share_axes(
        &mut self,
        ambient: &TextStyle,
        other: &mut Self,
        other_ambient: &TextStyle,
    ) {
        share(
            &mut self.color,
            ambient.color,
            &mut other.color,
            other_ambient.color,
        );
        share(
            &mut self.font_size,
            ambient.font_size,
            &mut other.font_size,
            other_ambient.font_size,
        );
        share(
            &mut self.line_height_factor,
            ambient.line_height_factor,
            &mut other.line_height_factor,
            other_ambient.line_height_factor,
        );
        share(
            &mut self.family,
            ambient.family,
            &mut other.family,
            other_ambient.family,
        );
        share(
            &mut self.weight,
            ambient.weight,
            &mut other.weight,
            other_ambient.weight,
        );
        share(
            &mut self.slant,
            ambient.slant,
            &mut other.slant,
            other_ambient.slant,
        );
    }

    /// Chainable single-axis override, the counterpart of
    /// [`TextStyle::with_font_size`].
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size = Some(px);
        self
    }

    /// [`Self::with_font_size`] for the color axis.
    #[inline]
    pub const fn with_color(mut self, color: RgbaF32) -> Self {
        self.color = Some(color);
        self
    }

    /// [`Self::with_font_size`] for the line-height axis.
    #[inline]
    pub const fn with_line_height_factor(mut self, factor: f32) -> Self {
        self.line_height_factor = Some(factor);
        self
    }

    /// [`Self::with_font_size`] for the family axis.
    #[inline]
    pub const fn with_family(mut self, family: FontFamily) -> Self {
        self.family = Some(family);
        self
    }

    /// [`Self::with_font_size`] for the weight axis.
    #[inline]
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// [`Self::with_font_size`] for the slant axis.
    #[inline]
    pub const fn with_slant(mut self, slant: FontSlant) -> Self {
        self.slant = Some(slant);
        self
    }

    /// Whether this set names no axis.
    pub(crate) const fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.font_size.is_none()
            && self.line_height_factor.is_none()
            && self.family.is_none()
            && self.weight.is_none()
            && self.slant.is_none()
    }

    /// Whether every named axis can stand in a face the shaper accepts. With
    /// both size and leading named the base cannot change the line height, so
    /// the whole face is checked.
    fn metrics_valid(&self) -> bool {
        match (self.font_size, self.line_height_factor) {
            (Some(_), Some(_)) => self
                .apply(&TextStyle::stock(ColorScheme::Dark))
                .metrics_valid(),
            (Some(px), None) => domain::is_font_length(px),
            (None, Some(factor)) => factor.is_finite() && factor > 0.0,
            (None, None) => true,
        }
    }
}

/// Each axis as [`TextStyle`] blends it: the color, the size and the leading blend, and the face
/// switches at `t = 0.5`. An axis one end names and the other leaves to the ambient style switches
/// at `t = 0.5` as well, as the overrides alone hold no ambient to blend from; a whole theme's lerp
/// first gives that end its ambient's value (`Theme::share_text_axes`).
impl Lerp for TextStyleOverrides {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let (a, b) = (*a, *b);
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        Self {
            color: lerp_axis(a.color, b.color, t),
            font_size: lerp_axis(a.font_size, b.font_size, t),
            line_height_factor: lerp_axis(a.line_height_factor, b.line_height_factor, t),
            family: switch_axis(a.family, b.family, t),
            weight: switch_axis(a.weight, b.weight, t),
            slant: switch_axis(a.slant, b.slant, t),
        }
    }
}

/// Both ends blended where both name the axis, else switched at `t = 0.5`.
#[inline]
fn lerp_axis<T: Lerp>(a: Option<T>, b: Option<T>, t: f32) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(T::lerp(&a, &b, t)),
        (a, b) => switch_axis(a, b, t),
    }
}

/// The axis on the end that lacks it, from that end's ambient value.
#[inline]
fn share<T>(a: &mut Option<T>, ambient_a: T, b: &mut Option<T>, ambient_b: T) {
    match (a.is_some(), b.is_some()) {
        (false, true) => *a = Some(ambient_a),
        (true, false) => *b = Some(ambient_b),
        _ => {}
    }
}

#[inline]
fn switch_axis<T>(a: Option<T>, b: Option<T>, t: f32) -> Option<T> {
    if t < 0.5 { a } else { b }
}

/// [`TextStyleOverrides`] as it arrives off the wire, read straight into a `TextStyleOverrides`
/// by serde's remote derive before the metrics check, as [`TextStyle`] reads its own.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "TextStyleOverrides")]
struct UncheckedTextStyleOverrides {
    color: Option<RgbaF32>,
    #[serde(
        default,
        deserialize_with = "crate::primitives::math::domain::file::optional_font_size"
    )]
    font_size: Option<f32>,
    line_height_factor: Option<f32>,
    family: Option<FontFamily>,
    weight: Option<FontWeight>,
    slant: Option<FontSlant>,
}

impl<'de> serde::Deserialize<'de> for TextStyleOverrides {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let overrides = UncheckedTextStyleOverrides::deserialize(deserializer)?;
        if !overrides.metrics_valid() {
            return Err(D::Error::custom(domain::FONT_LENGTH_RULE));
        }
        Ok(overrides)
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::FontFamily;
    use crate::theme::color_scheme::ColorScheme;
    use crate::theme::text_style::TextStyle;
    use crate::theme::text_style_overrides::TextStyleOverrides;

    /// The family chainers set that axis only, on a style and an override set alike: the override replaces the base's `SANS` and keeps its size.
    #[test]
    fn with_family_sets_only_the_family_axis() {
        let base = TextStyle::stock(ColorScheme::Dark);
        assert_eq!(base.family, FontFamily::SANS);
        let mono = base.with_family(FontFamily::MONO);
        assert_eq!(mono.family, FontFamily::MONO);
        assert_eq!(mono.font_size, base.font_size);

        let over = TextStyleOverrides::NONE.with_family(FontFamily::MONO);
        assert_eq!(over.family, Some(FontFamily::MONO));
        assert_eq!(over.font_size, None);
        let applied = over.apply(&base);
        assert_eq!(applied.family, FontFamily::MONO);
        assert_eq!(applied.font_size, base.font_size);
    }
}
