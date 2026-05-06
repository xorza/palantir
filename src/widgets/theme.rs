use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::shape::Shape;
use crate::ui::Ui;

// Default palette: Ayu Mirage High Contrast. Mirrors
// `assets/reference-palette.toml` — that file is the hand-edited source
// of truth; these consts are the compile-time copy used to build the
// framework defaults. Keep in sync when the palette changes.
mod palette {
    use crate::primitives::color::Color;
    // backgrounds
    pub const TERMINAL_BG: Color = Color::hex(0x1a1a1a);
    pub const ELEM: Color = Color::hex(0x343434);
    pub const ELEM_HOVER: Color = Color::hex(0x3e3e3e);
    pub const ELEM_ACTIVE: Color = Color::hex(0x4b4b4b);
    // borders
    pub const BORDER_FOCUSED: Color = Color::hex(0x105577);
    // text
    pub const TEXT: Color = Color::hex(0xffffff);
    pub const TEXT_MUTED: Color = Color::hex(0xaaaaa8);
    pub const TEXT_DISABLED: Color = Color::hex(0x878a8d);
    // accent
    pub const ACCENT: Color = Color::hex(0x9adbfb);
}

/// Paint data shared by container widgets (`Frame`, `Panel`, `Grid`)
/// and per-state widget Visuals: fill colour, optional stroke, and
/// corner radii. Default is transparent fill / no stroke / zero radius
/// — emitting nothing — so a container that never sets any of these
/// adds no shape to the tree (`Ui::add_shape` filters no-op shapes).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Background {
    pub fill: Color,
    pub stroke: Option<Stroke>,
    pub radius: Corners,
}

impl Background {
    pub(crate) fn add_to(&self, ui: &mut Ui) {
        ui.add_shape(Shape::RoundedRect {
            radius: self.radius,
            fill: self.fill,
            stroke: self.stroke,
        });
    }
}

/// Global theme. Aggregates per-widget themes. Widgets opt in by reading
/// from `Ui::theme`.
///
/// The framework does not auto-dim disabled subtrees — that's an
/// app/theme concern. Widgets that want disabled-state visuals read the
/// disabled flag themselves and pick their own colors at recording
/// time.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    pub button: ButtonTheme,
    pub scrollbar: ScrollbarTheme,
    pub text_edit: TextEditTheme,
    pub text: TextStyle,
    /// Window/swapchain clear color. Hosts pass to `WgpuBackend::submit`.
    pub window_clear: Color,
    /// Default background for container widgets (`Frame`, `Panel`,
    /// `Grid`) when the call site didn't pass `.background(...)`.
    /// `None` (the default) means containers paint nothing — original
    /// behavior. Setting `Some(...)` lights up every unstyled container
    /// at once, useful for prototyping or for showcasing layout (set
    /// a thin stroke and you can see every panel boundary without
    /// editing each call site).
    pub panel: Option<Background>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            scrollbar: ScrollbarTheme::default(),
            text_edit: TextEditTheme::default(),
            text: TextStyle::default(),
            window_clear: palette::TERMINAL_BG,
            panel: None,
        }
    }
}

/// Default text-rendering inputs grouped together so apps can swap the
/// whole "text look" with one assignment, and so future axes (font
/// family, weight, italic, letter-spacing) extend a single struct
/// rather than scattering across [`Theme`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
            color: palette::TEXT,
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
    #[allow(dead_code)] // first reader is the v1.1 drag/hover branch
    pub thumb_hover: Color,
    /// Thumb fill while drag-captured. Read once drag-to-pan lands.
    #[allow(dead_code)] // first reader is the v1.1 drag/hover branch
    pub thumb_active: Color,
    /// Corner radius applied to track and thumb. `width / 2` = pill.
    pub radius: f32,
}

impl Default for ScrollbarTheme {
    fn default() -> Self {
        // Ayu doesn't define scrollbar colors directly. Use TEXT_MUTED
        // at decreasing translucency for idle / hover / active so the
        // bar reads as a soft overlay matching the palette's
        // muted-text gray rather than pure black.
        let thumb = |alpha: f32| {
            let m = palette::TEXT_MUTED;
            Color::linear_rgba(m.r, m.g, m.b, alpha)
        };
        Self {
            width: 8.0,
            gap: 4.0,
            min_thumb_px: 24.0,
            track: Color::TRANSPARENT,
            thumb: thumb(0.45),
            thumb_hover: thumb(0.65),
            thumb_active: thumb(0.85),
            radius: 4.0,
        }
    }
}

/// Paint settings for one [`crate::TextEdit`] state — `normal` (the
/// idle / unfocused state), `focused`, or `disabled`. Same shape as
/// [`ButtonStateStyle`] and follows the same inheritance rule:
/// `Some(x)` overrides; `None` inherits the framework default for that
/// field. `background = None` inherits [`Background::default`] (paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`Theme::text`], so an app changing `theme.text.color`
/// moves every editor's buffer text along with every button label.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextEditStateStyle {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Three-state TextEdit theme. The leaf type ([`TextEditStateStyle`])
/// lives next to it; widget reads `theme.{normal,focused,disabled}`
/// based on `Element::disabled` and focus.
///
/// State-independent fields (`caret`, `caret_width`, `placeholder`,
/// `selection`, `padding`, `margin`) live flat on the theme — they
/// aren't state-varying in any plausible v1.x design.
///
/// `padding`/`margin` apply when the user didn't call
/// `.padding(...)` / `.margin(...)` on the builder. The "user didn't
/// override" check is `element.padding == Spacing::ZERO` — so if you
/// want a TextEdit with no padding while the theme has padding, set a
/// custom theme rather than passing zero.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextEditTheme {
    pub normal: TextEditStateStyle,
    pub focused: TextEditStateStyle,
    pub disabled: TextEditStateStyle,
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
    /// Default padding inside the editor (around the buffer text).
    /// Applied at `show()` time when the builder hasn't set padding.
    pub padding: Spacing,
    /// Default margin around the editor.
    pub margin: Spacing,
}

impl Default for TextEditTheme {
    fn default() -> Self {
        let radius = Corners::all(4.0);
        // Palette BORDER is ~2% above SURFACE — invisible. Derive edge from TEXT_MUTED alpha.
        let m = palette::TEXT_MUTED;
        let edge = Color::linear_rgba(m.r, m.g, m.b, 0.18);
        let normal_bg = Background {
            fill: palette::ELEM_HOVER,
            stroke: Some(Stroke {
                width: 1.0,
                color: edge,
            }),
            radius,
        };
        let focused_bg = Background {
            fill: palette::ELEM_HOVER,
            stroke: Some(Stroke {
                width: 1.5,
                color: palette::BORDER_FOCUSED,
            }),
            radius,
        };
        let disabled_bg = Background {
            fill: palette::ELEM,
            stroke: Some(Stroke {
                width: 1.0,
                color: edge,
            }),
            radius,
        };
        // Selection = accent at ~25% alpha — readable wash that doesn't
        // obscure the glyphs underneath.
        let acc = palette::ACCENT;
        let selection = Color::linear_rgba(acc.r, acc.g, acc.b, 0.25);
        Self {
            normal: TextEditStateStyle {
                background: Some(normal_bg),
                text: None,
            },
            focused: TextEditStateStyle {
                background: Some(focused_bg),
                text: None,
            },
            disabled: TextEditStateStyle {
                background: Some(disabled_bg),
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            placeholder: palette::TEXT_MUTED,
            caret: palette::TEXT,
            caret_width: 1.5,
            selection,
            padding: Spacing::xy(8.0, 6.0),
            margin: Spacing::ZERO,
        }
    }
}

/// Paint settings for one [`crate::Button`] state — `normal`,
/// `hovered`, `pressed`, or `disabled`. Same shape as
/// [`TextEditStateStyle`] and follows the same inheritance rule:
/// `Some(x)` overrides; `None` inherits the framework default for that
/// field. `background = None` inherits [`Background::default`] (paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`Theme::text`], so an app changing `theme.text.color`
/// moves every button label that didn't override it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ButtonStateStyle {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Four-state button theme. The leaf type ([`ButtonStateStyle`]) lives
/// next to it; widget reads `theme.{normal,hovered,pressed,disabled}`
/// based on the live response state and `Element::disabled`.
///
/// `padding`/`margin` apply when the user didn't call `.padding(...)`
/// / `.margin(...)` on the builder. The "user didn't override" check
/// is `element.padding == Spacing::ZERO` — so if you want a button
/// with no padding while the theme has padding, set a custom theme
/// rather than passing zero.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ButtonTheme {
    pub normal: ButtonStateStyle,
    pub hovered: ButtonStateStyle,
    pub pressed: ButtonStateStyle,
    pub disabled: ButtonStateStyle,
    /// Default padding inside the button (around the label).
    /// Applied at `show()` time when the builder hasn't set padding.
    pub padding: Spacing,
    /// Default margin around the button.
    pub margin: Spacing,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        // Buttons map to the palette's clickable-surface family:
        // ELEM / ELEM_HOVER / ELEM_ACTIVE. Disabled keeps the same
        // ELEM fill but swaps text to TEXT_DISABLED. `text: None` on
        // active states means "inherit Theme::text" — bumping
        // `theme.text.color` recolors active button labels. The
        // historical 4 px radius is retained.
        // Resting state at ELEM_HOVER tier; soft TEXT_MUTED-alpha edge (palette BORDER is invisible).
        let m = palette::TEXT_MUTED;
        let edge = Color::linear_rgba(m.r, m.g, m.b, 0.18);
        let bg = |fill: Color| -> Option<Background> {
            Some(Background {
                fill,
                stroke: Some(Stroke {
                    width: 1.0,
                    color: edge,
                }),
                radius: Corners::all(4.0),
            })
        };
        // Pressed = hovered fill + focused stroke (palette has no further fill tier).
        let pressed_bg = Background {
            fill: palette::ELEM_ACTIVE,
            stroke: Some(Stroke {
                width: 1.0,
                color: palette::BORDER_FOCUSED,
            }),
            radius: Corners::all(4.0),
        };
        Self {
            normal: ButtonStateStyle {
                background: bg(palette::ELEM_HOVER),
                text: None,
            },
            hovered: ButtonStateStyle {
                background: bg(palette::ELEM_ACTIVE),
                text: None,
            },
            pressed: ButtonStateStyle {
                background: Some(pressed_bg),
                text: None,
            },
            disabled: ButtonStateStyle {
                background: bg(palette::ELEM),
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            padding: Spacing::xy(12.0, 6.0),
            margin: Spacing::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_roundtrips_through_toml() {
        let theme = Theme::default();
        let serialized = toml::to_string_pretty(&theme).expect("serialize");
        let parsed: Theme = toml::from_str(&serialized).expect("parse");
        let reserialized = toml::to_string_pretty(&parsed).expect("re-serialize");
        // Comparing serialized strings rather than `Theme == Theme`:
        // `ScrollbarTheme` deliberately doesn't derive `PartialEq`,
        // and forcing it everywhere would be theme-API churn. String
        // equality is just as strong — every field round-trips.
        assert_eq!(serialized, reserialized);
    }
}
