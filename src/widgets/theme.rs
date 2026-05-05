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
#[derive(Clone, Debug)]
pub struct Theme {
    pub button: ButtonTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    /// Line-height-to-font-size ratio used by every text-rendering
    /// widget (Button label, [`crate::Text`], [`crate::TextEdit`]).
    /// Drives the shaper's leading and the caret rect height (locked
    /// together via `Shape::Text.line_height_px`). Default matches
    /// cosmic-text's natural leading
    /// ([`crate::text::LINE_HEIGHT_MULT`], 1.2). Apps that want a
    /// different global look set this once at startup; per-widget
    /// override on TextEdit lives on the builder
    /// (`TextEdit::line_height_mult`).
    pub line_height_mult: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            scrollbar: ScrollbarTheme::default(),
            text_edit: TextEditTheme::default(),
            line_height_mult: crate::text::LINE_HEIGHT_MULT,
        }
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
/// each frame; per-widget overrides via [`crate::TextEdit::style`].
/// v1 has unfocused/focused background + stroke pairs, a caret color,
/// and a selection color (selection rendering is deferred but the slot
/// exists so a future enable doesn't require a theme change).
#[derive(Clone, Debug)]
pub struct TextEditTheme {
    pub background: Color,
    pub background_focused: Color,
    pub stroke: Option<Stroke>,
    pub stroke_focused: Option<Stroke>,
    pub radius: Corners,
    pub text: Color,
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
    /// Default font size used for the buffer when the widget builder
    /// doesn't override it. Matches `Button`'s historical 16 px.
    pub size_px: f32,
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
            text: Color::WHITE,
            placeholder: Color::rgba(1.0, 1.0, 1.0, 0.40),
            caret: Color::WHITE,
            caret_width: 1.5,
            selection: Color::rgba(0.30, 0.52, 0.92, 0.40),
            size_px: 16.0,
        }
    }
}
