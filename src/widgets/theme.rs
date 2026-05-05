use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::stroke::Stroke;
use crate::widgets::button::ButtonTheme;

/// Global theme. Aggregates per-widget themes. Widgets opt in by reading
/// from `Ui::theme`.
///
/// The framework does not auto-dim disabled subtrees — that's an
/// app/theme concern. Widgets that want disabled-state visuals read the
/// disabled flag themselves and pick their own colors at recording
/// time.
#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub button: ButtonTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    /// Global text defaults (font size, color, leading) that every
    /// text-rendering widget falls back to when its builder didn't set
    /// a per-widget override. See [`TextStyle`].
    pub text: TextStyle,
}

/// Default text-rendering inputs grouped together so apps can swap the
/// whole "text look" with one assignment, and so future axes (font
/// family, weight, italic, letter-spacing) extend a single struct
/// rather than scattering across [`Theme`].
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    /// Default font size in logical px. Button labels read this
    /// directly; [`crate::Text`] / [`crate::TextEdit`] fall back to it
    /// when their builder didn't set a size.
    pub font_size_px: f32,
    /// Default fill color for [`crate::Text`] runs that didn't call
    /// `.color(...)`. Button / TextEdit have their own state-dependent
    /// colors on their respective themes and don't read this.
    pub color: Color,
    /// Line-height-to-font-size ratio. Drives the shaper's leading and
    /// the caret rect height (locked together via
    /// `Shape::Text.line_height_px`). Default matches cosmic-text's
    /// natural leading ([`crate::text::LINE_HEIGHT_MULT`], 1.2). Per-
    /// widget override on TextEdit lives on the builder
    /// (`TextEdit::line_height_mult`).
    pub line_height_mult: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            color: Color::WHITE,
            line_height_mult: crate::text::LINE_HEIGHT_MULT,
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

/// Visuals for [`crate::Scroll`] reservation-layout scrollbars. When
/// content overflows on a panned axis, the widget reserves `width`
/// of padding on that axis's far edge; the bar paints in the reserved
/// strip — beside the visible content, never on top of it. Track +
/// thumb are filled rounded rects. v1 has no hover/active states (no
/// drag interaction yet), so `thumb` is the only color used today;
/// the slots exist so adding drag can light them up without an API
/// change.
#[derive(Clone, Debug)]
pub struct ScrollbarTheme {
    /// Cross-axis thickness of the bar in logical px.
    pub width: f32,
    /// Empty padding strip between content and the bar. Reserved
    /// alongside `width` (total reservation = `width + gap`) but
    /// painted as nothing — pure breathing room so the bar doesn't
    /// touch the visible content.
    pub gap: f32,
    /// Floor for the thumb's main-axis length so a tiny `viewport /
    /// content` ratio doesn't produce an ungrabbable nub.
    pub min_thumb_px: f32,
    /// Track background. `Color::TRANSPARENT` = pure overlay (only the
    /// thumb is visible) — the macOS-style default.
    pub track: Color,
    /// Idle thumb fill.
    pub thumb: Color,
    /// Thumb fill on hover. Read once hover-state on bar leaves lands
    /// (v1.1, alongside drag).
    pub thumb_hover: Color,
    /// Thumb fill while drag-captured. Read once drag-to-pan lands.
    pub thumb_active: Color,
    /// Corner radius applied to track and thumb. `width / 2` = pill.
    pub radius: f32,
}

impl Default for ScrollbarTheme {
    fn default() -> Self {
        Self {
            width: 8.0,
            gap: 4.0,
            min_thumb_px: 24.0,
            track: Color::TRANSPARENT,
            thumb: Color::rgba(0.0, 0.0, 0.0, 0.55),
            thumb_hover: Color::rgba(0.0, 0.0, 0.0, 0.7),
            thumb_active: Color::rgba(0.0, 0.0, 0.0, 0.85),
            radius: 4.0,
        }
    }
}

/// Visuals for [`crate::TextEdit`]. Read from [`Theme::text_edit`]
/// each frame; per-widget overrides via [`crate::TextEdit::style`] —
/// all-or-nothing, the whole theme is replaced when set.
///
/// `text` is a [`TextStyle`] bundle so font size, color, and leading
/// for the buffer text live in one slot rather than scattered fields.
#[derive(Clone, Debug)]
pub struct TextEditTheme {
    pub background: Color,
    pub background_focused: Color,
    pub stroke: Option<Stroke>,
    pub stroke_focused: Option<Stroke>,
    pub radius: Corners,
    /// Font/leading/color for the buffer text in the *unfocused-or-
    /// focused-non-empty* state. Placeholder uses [`Self::placeholder`]
    /// instead.
    pub text: TextStyle,
    pub placeholder: Color,
    pub caret: Color,
    /// Width of the caret rect in logical px. The caret is painted as
    /// a thin Overlay rect at the caret's prefix-x; one pixel reads as
    /// a hairline, two as a chunkier i-beam. Default 1.5 px.
    pub caret_width: f32,
    /// Selection highlight fill. Unused in v1 (no selection ops yet)
    /// but kept on the theme so enabling selection later doesn't
    /// require a theme migration.
    pub selection: Color,
}

impl Default for TextEditTheme {
    fn default() -> Self {
        Self {
            background: Color::rgb(0.10, 0.12, 0.16),
            background_focused: Color::rgb(0.13, 0.16, 0.22),
            stroke: Some(Stroke {
                width: 1.0,
                color: Color::rgba(1.0, 1.0, 1.0, 0.10),
            }),
            stroke_focused: Some(Stroke {
                width: 1.5,
                color: Color::rgb(0.30, 0.52, 0.92),
            }),
            radius: Corners::all(4.0),
            text: TextStyle::default(),
            placeholder: Color::rgba(1.0, 1.0, 1.0, 0.40),
            caret: Color::WHITE,
            caret_width: 1.5,
            selection: Color::rgba(0.30, 0.52, 0.92, 0.40),
        }
    }
}
