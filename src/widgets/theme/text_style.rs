//! The font, size, weight, colour and leading a run of text is shaped and
//! painted with.

use crate::primitives::paint::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::widgets::theme::palette::Palette;

/// Default [`TextStyle::line_height_factor`]. A widget convention; the shaper
/// takes resolved pixels and never sees a multiplier.
pub(crate) const LINE_HEIGHT_MULT: f32 = 1.2;

/// Default text-rendering inputs, grouped so an app can swap the whole text
/// look with one assignment.
///
/// `color` interpolates; the rest are `#[animate(snap)]` because animating
/// size invalidates the shape cache every frame.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Animatable,
)]
#[serde(try_from = "UncheckedTextStyle")]
#[must_use]
pub struct TextStyle {
    /// Default font size in logical px; [`crate::Text`] and
    /// [`crate::TextEdit`] fall back to it.
    #[animate(snap)]
    pub font_size: f32,
    /// Default ink for [`crate::Text`] and for every widget look state that
    /// leaves `color` unset.
    pub color: RgbaF32,
    /// Line-height-to-font-size ratio, driving shaper leading and caret
    /// height. Default is cosmic-text's natural 1.2.
    #[animate(snap)]
    pub line_height_factor: f32,
    /// Font family. Default [`FontFamily::SANS`] is bundled Inter.
    #[animate(snap)]
    pub family: FontFamily,
    /// Font weight on the CSS 1–1000 scale. Default [`FontWeight::REGULAR`].
    #[animate(snap)]
    pub weight: FontWeight,
    /// Upright or italic; a slant is synthesized where the family has no
    /// italic face. Default [`FontSlant::Normal`].
    #[animate(snap)]
    pub slant: FontSlant,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            color: Palette::DEFAULT.text,
            line_height_factor: LINE_HEIGHT_MULT,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }
    }
}

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
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "UncheckedTextStyleOverrides")]
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

    /// Chainable single-axis override, the counterpart of
    /// [`TextStyle::with_font_size`].
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size = Some(px);
        self
    }

    /// [`Self::with_font_size`] for the colour axis.
    #[inline]
    pub const fn with_color(mut self, c: RgbaF32) -> Self {
        self.color = Some(c);
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
            (Some(_), Some(_)) => self.apply(&TextStyle::default()).metrics_valid(),
            (Some(px), None) => GlyphFont::length_is_valid(px),
            (None, Some(factor)) => factor.is_finite() && factor > 0.0,
            (None, None) => true,
        }
    }
}

impl TextStyle {
    pub(crate) fn metrics_valid(&self) -> bool {
        GlyphFont::metrics_are_valid(self.font_size, self.line_height_for(self.font_size))
    }

    /// This style as the face the shaper is asked for. Widgets go through
    /// here rather than pairing `font_size` with a separately computed line
    /// height, so the two cannot disagree.
    #[inline]
    pub fn font(&self) -> GlyphFont {
        GlyphFont {
            size: self.font_size,
            line_height: self.line_height_for(self.font_size),
            family: self.family,
            weight: self.weight,
            slant: self.slant,
        }
    }

    /// Absolute line height in px for text at `font_size`: the one owner of
    /// the `line_height_factor` formula.
    ///
    /// Answers on the 1/64-px grid the shaper's cache keys hold: 16 px at 1.2
    /// leads at 19.203125, not 19.2.
    #[inline]
    pub fn line_height_for(&self, font_size: f32) -> f32 {
        TextShapeKey::leading_on_grid(font_size * self.line_height_factor)
    }

    /// Chainable single-axis tweak: `theme.text.with_font_size(14.0)`.
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size = px;
        self
    }

    /// [`Self::with_font_size`] for the colour axis.
    #[inline]
    pub const fn with_color(mut self, c: RgbaF32) -> Self {
        self.color = c;
        self
    }

    /// [`Self::with_font_size`] for the line-height axis.
    #[inline]
    pub const fn with_line_height_factor(mut self, factor: f32) -> Self {
        self.line_height_factor = factor;
        self
    }

    /// [`Self::with_font_size`] for the family axis.
    #[inline]
    pub const fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    /// [`Self::with_font_size`] for the weight axis.
    #[inline]
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// [`Self::with_font_size`] for the slant axis.
    #[inline]
    pub const fn with_slant(mut self, slant: FontSlant) -> Self {
        self.slant = slant;
        self
    }

    /// Shorthand for `.with_weight(FontWeight::BOLD)`.
    #[inline]
    pub const fn bold(self) -> Self {
        self.with_weight(FontWeight::BOLD)
    }

    /// Shorthand for `.with_slant(FontSlant::Italic)`.
    #[inline]
    pub const fn italic(self) -> Self {
        self.with_slant(FontSlant::Italic)
    }
}

/// [`TextStyle`] as it arrives off the wire, before the metrics check.
/// A theme file is untrusted: a non-finite or non-positive size would reach
/// the shaper as a face it cannot resolve.
#[derive(Debug, serde::Deserialize)]
struct UncheckedTextStyle {
    font_size: f32,
    color: RgbaF32,
    line_height_factor: f32,
    family: FontFamily,
    weight: FontWeight,
    slant: FontSlant,
}

impl TryFrom<UncheckedTextStyle> for TextStyle {
    type Error = &'static str;

    fn try_from(style: UncheckedTextStyle) -> Result<Self, Self::Error> {
        let style = Self {
            font_size: style.font_size,
            color: style.color,
            line_height_factor: style.line_height_factor,
            family: style.family,
            weight: style.weight,
            slant: style.slant,
        };
        if !style.metrics_valid() {
            return Err(GlyphFont::METRICS_ERROR);
        }
        Ok(style)
    }
}

/// [`TextStyleOverrides`] as it arrives off the wire, before the metrics check.
#[derive(Debug, serde::Deserialize)]
struct UncheckedTextStyleOverrides {
    color: Option<RgbaF32>,
    font_size: Option<f32>,
    line_height_factor: Option<f32>,
    family: Option<FontFamily>,
    weight: Option<FontWeight>,
    slant: Option<FontSlant>,
}

impl TryFrom<UncheckedTextStyleOverrides> for TextStyleOverrides {
    type Error = &'static str;

    fn try_from(overrides: UncheckedTextStyleOverrides) -> Result<Self, Self::Error> {
        let overrides = Self {
            color: overrides.color,
            font_size: overrides.font_size,
            line_height_factor: overrides.line_height_factor,
            family: overrides.family,
            weight: overrides.weight,
            slant: overrides.slant,
        };
        if !overrides.metrics_valid() {
            return Err(GlyphFont::METRICS_ERROR);
        }
        Ok(overrides)
    }
}
