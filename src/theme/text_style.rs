//! The font, size, weight, color and leading a run of text is shaped and
//! painted with.

use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::text::extent;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use serde::de::Error as _;

/// Default [`TextStyle::line_height_factor`]. A widget convention; the shaper
/// takes resolved pixels and never sees a multiplier.
pub(crate) const LINE_HEIGHT_FACTOR: f32 = 1.2;

/// Default text-rendering inputs, grouped so an app can swap the whole text
/// look with one assignment.
///
/// `color` interpolates. `font_size` and `line_height_factor` are `#[animate(lerp_only)]`: a look's
/// fade takes them at once, as each step of a size reshapes the text and a hover would pay that
/// on every frame of it, while a theme's lerp blends them, its layout moving with it. The face
/// is `#[animate(snap)]`, with no blend.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    serde::Serialize,
    palantir_derive::Lerp,
    palantir_derive::Animatable,
)]
#[must_use]
pub struct TextStyle {
    /// Default font size in logical px; [`crate::Text`] and
    /// [`crate::TextEdit`] fall back to it. Default 14, the body size of the
    /// Fluent 2 type ramp (<https://fluent2.microsoft.design/typography>): a
    /// desktop control's label, where the web's 16 reads large beside a 4 px
    /// grid's insets.
    #[animate(lerp_only)]
    pub font_size: f32,
    /// Default ink for [`crate::Text`] and for every widget look state that
    /// leaves `color` unset.
    pub color: RgbaF32,
    /// Line-height-to-font-size ratio, driving shaper leading and caret
    /// height. Default is cosmic-text's natural 1.2.
    #[animate(lerp_only)]
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

impl TextStyle {
    /// A theme's body text over `p` and `m`: [`Metrics`]' body size in the palette's ink, in the
    /// bundled sans at its regular weight, upright.
    pub const fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            font_size: m.text.body,
            color: p.text,
            line_height_factor: LINE_HEIGHT_FACTOR,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }
    }

    /// The built-in body text of `scheme`: its built-in palette over [`Metrics::DEFAULT`], as
    /// [`Theme::stock`](crate::Theme::stock) holds it in `text`. The one slot with a `stock` of its
    /// own: a `const` base an app's styles build on, where the theme's is built whole at run time.
    pub const fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }

    pub(crate) fn metrics_valid(&self) -> bool {
        GlyphFont::metrics_are_valid(self.font_size, self.line_height())
    }

    /// This style as the face the shaper is asked for. Widgets go through
    /// here rather than pairing `font_size` with a separately computed line
    /// height, so the two cannot disagree.
    #[inline]
    pub fn font(&self) -> GlyphFont {
        GlyphFont {
            font_size: self.font_size,
            line_height: self.line_height(),
            family: self.family,
            weight: self.weight,
            slant: self.slant,
        }
    }

    /// Absolute line height in px of this style's text: the one owner of the
    /// `line_height_factor` formula. Another size's reads through
    /// [`Self::with_font_size`].
    ///
    /// Answers on the 1/64-px grid the shaper's cache keys hold: 16 px at 1.2
    /// leads at 19.203125, not 19.2.
    #[inline]
    pub fn line_height(&self) -> f32 {
        TextShapeKey::leading_on_grid(self.font_size * self.line_height_factor)
    }

    /// The height `line_count` lines of this style take as palantir lays out a block of text:
    /// [`Self::line_height`] times the count, rounded up to whole pixels once for the block. What
    /// a `Text` of that many lines measures, and what a `TextEdit` floors an empty line at, so a
    /// custom widget sized by it sits flush with them: 16 px at 1.2 takes 20 px a line and 39 for
    /// two, not 40. [`Self::line_height`] stays the step from one line to the next.
    #[inline]
    pub fn block_height(&self, line_count: usize) -> f32 {
        extent::block_height(self.line_height(), line_count)
    }

    /// Chainable single-axis tweak: `theme.text.with_font_size(14.0)`.
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size = px;
        self
    }

    /// [`Self::with_font_size`] for the color axis.
    #[inline]
    pub const fn with_color(mut self, color: RgbaF32) -> Self {
        self.color = color;
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

/// [`TextStyle`] as it arrives off the wire, read straight into a `TextStyle` by serde's remote
/// derive before the metrics check, as [`Theme`](crate::Theme) reads its own. A theme file is
/// untrusted: a non-finite or non-positive size would reach the shaper as a face it cannot
/// resolve.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "TextStyle")]
struct UncheckedTextStyle {
    #[serde(deserialize_with = "crate::primitives::math::domain::file::font_size")]
    font_size: f32,
    color: RgbaF32,
    line_height_factor: f32,
    family: FontFamily,
    weight: FontWeight,
    slant: FontSlant,
}

impl<'de> serde::Deserialize<'de> for TextStyle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let style = UncheckedTextStyle::deserialize(deserializer)?;
        if !style.metrics_valid() {
            return Err(D::Error::custom(domain::FONT_LENGTH_RULE));
        }
        Ok(style)
    }
}
