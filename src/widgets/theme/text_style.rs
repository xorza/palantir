//! The font, size, weight, colour and leading a run of text is shaped and
//! painted with — the vocabulary every other theme carries a copy of.

use crate::primitives::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
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
    /// natural leading (1.2). A *look* overrides it by carrying a whole
    /// [`TextStyle`] in its `text` slot, since a look either replaces
    /// every text axis or inherits every one. A *caller* overrides it
    /// alone through [`TextStyleOverrides`].
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
    /// Upright or italic. Default [`FontSlant::Normal`]; set
    /// [`FontSlant::Italic`] (or call [`Self::italic`]) to shape against
    /// the family's italic face, or a synthesized slant where it has
    /// none.
    #[animate(snap)]
    pub slant: FontSlant,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            color: Palette::DEFAULT.text,
            line_height_mult: LINE_HEIGHT_MULT,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }
    }
}

/// Per-axis overrides folded onto a resolved [`TextStyle`].
///
/// What a text-rendering widget collects from its one-axis setters —
/// [`Text::color`](crate::Text::color),
/// [`TextEdit::font_size`](crate::TextEdit::font_size) and the rest — so
/// that a caller who wants one axis does not have to build a whole bundle.
/// A `None` field leaves the resolved style's own value standing.
///
/// One type rather than a set of fields per widget, so [`Text`](crate::Text)
/// and [`TextEdit`](crate::TextEdit) answer the same chain, and so a widget
/// of your own can offer it too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextStyleOverrides {
    pub color: Option<RgbaF32>,
    pub font_size_px: Option<f32>,
    pub line_height_mult: Option<f32>,
    pub family: Option<FontFamily>,
    pub weight: Option<FontWeight>,
    pub slant: Option<FontSlant>,
}

impl TextStyleOverrides {
    /// `base` with every axis this set names replaced.
    #[inline]
    pub fn apply(self, base: &TextStyle) -> TextStyle {
        TextStyle {
            font_size_px: self.font_size_px.unwrap_or(base.font_size_px),
            color: self.color.unwrap_or(base.color),
            line_height_mult: self.line_height_mult.unwrap_or(base.line_height_mult),
            family: self.family.unwrap_or(base.family),
            weight: self.weight.unwrap_or(base.weight),
            slant: self.slant.unwrap_or(base.slant),
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
    /// A builder that overrides one axis folds a [`TextStyleOverrides`]
    /// onto the style before it gets here, which is why there is no
    /// per-field variant.
    #[inline]
    pub fn font(&self) -> GlyphFont {
        GlyphFont {
            size_px: self.font_size_px,
            line_height_px: self.line_height_for(self.font_size_px),
            family: self.family,
            weight: self.weight,
            slant: self.slant,
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
    slant: FontSlant,
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
            slant: style.slant,
        };
        if !style.metrics_valid() {
            return Err(GlyphFont::METRICS_ERROR);
        }
        Ok(style)
    }
}
