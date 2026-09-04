//! The font, size, weight, colour and leading a run of text is shaped and
//! painted with — the vocabulary every other theme carries a copy of.

use crate::primitives::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::widgets::theme::palette::Palette;

/// Default [`TextStyle::line_height_mult`]: the leading widgets resolve
/// into the `line_height_px` they record, and so also the y-range a caret
/// spans.
///
/// A widget convention, not a shaping one — the shaper takes resolved
/// pixels off `ShapeRecord::Text` and never consults a multiplier.
pub(crate) const LINE_HEIGHT_MULT: f32 = 1.2;

/// Default text-rendering inputs grouped together so apps can swap the
/// whole "text look" with one assignment, and so future axes (italic,
/// letter-spacing) extend a single struct rather than scattering across
/// [`crate::Theme`].
///
/// `Animatable` derived: `color` interpolates; `font_size_px` and
/// `line_height_mult` are `#[animate(snap)]` because animating font
/// size invalidates the text-shape cache every frame and animating
/// leading doesn't read meaningfully.
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
pub struct TextStyle {
    /// Default font size in logical px. Button labels read this
    /// directly; [`crate::Text`] / [`crate::TextEdit`] fall back to it
    /// when their builder didn't set a size.
    #[animate(snap)]
    pub font_size_px: f32,
    /// Default fill color for [`crate::Text`] runs that didn't call
    /// `.color(...)`, and the ink a widget look inherits: `Button` and
    /// `TextEdit` carry a state-dependent `TextStyle` per state, and
    /// every state that leaves it `None` — which is every active one by
    /// default — resolves to this.
    pub color: RgbaF32,
    /// Line-height-to-font-size ratio. Drives the shaper's leading and
    /// the caret rect height (locked together via
    /// `ShapeRecord::Text.line_height_px`). Default matches cosmic-text's
    /// natural leading (1.2). A widget overrides it by carrying a whole
    /// [`TextStyle`] in its look's `text` slot, since a look either
    /// replaces every text axis or inherits every one.
    #[animate(snap)]
    pub line_height_mult: f32,
    /// Font family used for shaping. Default
    /// [`FontFamily::SANS`] resolves to bundled Inter; the debug
    /// `frame_stats` overlay overrides to [`FontFamily::MONO`].
    #[animate(snap)]
    pub family: FontFamily,
    /// Font weight used for shaping, on the CSS 1–1000 scale. Default
    /// [`FontWeight::REGULAR`]; set [`FontWeight::BOLD`] (or call
    /// [`Self::bold`]) to shape against the family's bold face.
    #[animate(snap)]
    pub weight: FontWeight,
    /// Upright or italic. Default [`FontStyle::Normal`]; set
    /// [`FontStyle::Italic`] (or call [`Self::italic`]) to shape against
    /// the family's italic face, or a synthesized slant where it has
    /// none.
    #[animate(snap)]
    pub style: FontStyle,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            color: Palette::DEFAULT.text,
            line_height_mult: LINE_HEIGHT_MULT,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            style: FontStyle::Normal,
        }
    }
}

impl TextStyle {
    pub(crate) fn metrics_valid(&self) -> bool {
        GlyphFont::metrics_are_valid(self.font_size_px, self.line_height_for(self.font_size_px))
    }

    /// This style as the face the shaper is asked for, at its own size.
    ///
    /// The bridge between the theme's spelling of a face — a size plus a
    /// *ratio* — and the shaper's, which wants leading resolved. Every
    /// widget that records text goes through here rather than pairing
    /// `font_size_px` with a separately-computed line height, so the two
    /// cannot arrive at the shaper disagreeing.
    ///
    /// A builder that overrides one field writes a struct update over
    /// this — `GlyphFont { weight: bold, ..style.font() }` — which is why
    /// there is no per-field variant here.
    #[inline]
    pub fn font(&self) -> GlyphFont {
        GlyphFont {
            size_px: self.font_size_px,
            line_height_px: self.line_height_for(self.font_size_px),
            family: self.family,
            weight: self.weight,
            style: self.style,
        }
    }

    /// Resolve the absolute line-height-in-px the shaper will use for
    /// text rendered at `font_size_px`. Single call site that owns the
    /// `line_height_mult` formula; widgets call this instead of doing
    /// `font_size * line_height_mult` inline so the formula can evolve
    /// (font-dependent leading, etc.) without a sweep through every
    /// text-rendering widget.
    #[inline]
    pub fn line_height_for(&self, font_size_px: f32) -> f32 {
        font_size_px * self.line_height_mult
    }

    /// Chainable single-axis tweak. Lets callers write
    /// `theme.text.with_font_size(14.0)` instead of `TextStyle {
    /// font_size_px: 14.0, ..theme.text }`. All widget style setters
    /// borrow a whole `TextStyle` (all-or-nothing), so the common case of
    /// "theme defaults, but smaller" goes through one of these.
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size_px = px;
        self
    }

    #[inline]
    pub const fn with_color(mut self, c: RgbaF32) -> Self {
        self.color = c;
        self
    }

    #[inline]
    pub const fn with_line_height_mult(mut self, mult: f32) -> Self {
        self.line_height_mult = mult;
        self
    }

    #[inline]
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    #[inline]
    pub const fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }

    /// Shorthand for `.with_weight(FontWeight::BOLD)`.
    #[inline]
    pub const fn bold(self) -> Self {
        self.with_weight(FontWeight::BOLD)
    }

    /// Shorthand for `.with_style(FontStyle::Italic)`.
    #[inline]
    pub const fn italic(self) -> Self {
        self.with_style(FontStyle::Italic)
    }
}

/// [`TextStyle`] as it arrives off the wire, before the metrics check.
///
/// Exists because a theme file is untrusted input: a non-finite or
/// non-positive size reaches the shaper as a face it cannot resolve, and
/// the failure surfaces frames later as text that measured to nothing.
/// [`TextStyle`]'s `#[serde(try_from)]` routes every deserialize through
/// this, so no path builds one without the check.
#[derive(Debug, serde::Deserialize)]
struct UncheckedTextStyle {
    font_size_px: f32,
    color: RgbaF32,
    line_height_mult: f32,
    family: FontFamily,
    weight: FontWeight,
    style: FontStyle,
}

impl TryFrom<UncheckedTextStyle> for TextStyle {
    type Error = &'static str;

    fn try_from(style: UncheckedTextStyle) -> Result<Self, Self::Error> {
        let style = Self {
            font_size_px: style.font_size_px,
            color: style.color,
            line_height_mult: style.line_height_mult,
            family: style.family,
            weight: style.weight,
            style: style.style,
        };
        if !style.metrics_valid() {
            return Err(GlyphFont::METRICS_ERROR);
        }
        Ok(style)
    }
}
