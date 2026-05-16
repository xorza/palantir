use crate::primitives::color::Color;
use crate::text::FontFamily;
use crate::widgets::theme::palette;

/// Default text-rendering inputs grouped together so apps can swap the
/// whole "text look" with one assignment, and so future axes (font
/// family, weight, italic, letter-spacing) extend a single struct
/// rather than scattering across [`crate::Theme`].
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
pub struct TextStyle {
    /// Default font size in logical px. Button labels read this
    /// directly; [`crate::Text`] / [`crate::TextEdit`] fall back to it
    /// when their builder didn't set a size.
    #[animate(snap)]
    pub font_size_px: f32,
    /// Default fill color for [`crate::Text`] runs that didn't call
    /// `.color(...)`. Button / TextEdit have their own state-dependent
    /// colors on their respective themes and don't read this.
    pub color: Color,
    /// Line-height-to-font-size ratio. Drives the shaper's leading and
    /// the caret rect height (locked together via
    /// `ShapeRecord::Text.line_height_px`). Default matches cosmic-text's
    /// natural leading ([`crate::text::LINE_HEIGHT_MULT`], 1.2). Per-
    /// widget override on TextEdit lives on the builder
    /// (`TextEdit::line_height_mult`).
    #[animate(snap)]
    pub line_height_mult: f32,
    /// Font family used for shaping. Default
    /// [`FontFamily::Sans`] resolves to bundled Inter; the debug
    /// `frame_stats` overlay overrides to [`FontFamily::Mono`].
    #[animate(snap)]
    pub family: FontFamily,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            color: palette::TEXT,
            line_height_mult: crate::text::LINE_HEIGHT_MULT,
            family: FontFamily::Sans,
        }
    }
}

impl TextStyle {
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
    /// font_size_px: 14.0, ..theme.text }`. All widget setters take a
    /// whole `TextStyle` (all-or-nothing), so the common case of
    /// "theme defaults, but smaller" goes through one of these.
    #[inline]
    pub const fn with_font_size(mut self, px: f32) -> Self {
        self.font_size_px = px;
        self
    }

    #[inline]
    pub const fn with_color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    #[inline]
    pub const fn with_line_height_mult(mut self, mult: f32) -> Self {
        self.line_height_mult = mult;
        self
    }
}
