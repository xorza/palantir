use crate::animation::{AnimSlot, AnimSpec};
use crate::input::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
pub use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::tree::widget_id::WidgetId;
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

/// Container chrome: optional paint plus optional clip behavior. The clip
/// reuses `paint.radius`, so paint and clip share one corner-radius source
/// of truth — no drift, no separate radius field. `Surface::apply_clip`
/// installs the clip flags onto an `Element`; the caller adds `paint` to
/// the node body via `paint.add_to(ui)`.
///
/// Usable by any container widget (`Panel`, `Grid`, `Scroll`, custom
/// widgets) — not panel-specific.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Surface {
    pub paint: Background,
    pub clip: ClipMode,
}

impl Surface {
    /// Pure scissor clip with no painted background — the canonical
    /// "scroll viewport" / "overflow:hidden" surface.
    pub const fn clip_rect() -> Self {
        Self {
            paint: Background {
                fill: Color::TRANSPARENT,
                stroke: None,
                radius: Corners::ZERO,
            },
            clip: ClipMode::Rect,
        }
    }

    /// Painted background plus scissor clip. Children of the panel are
    /// scissor-clipped to its rect; rounded paint corners are NOT
    /// stencil-clipped (use `.clip = ClipMode::Rounded` directly for
    /// that). Use this for "card with overflow hidden" where you don't
    /// need the stencil pass cost.
    pub const fn clip_rect_with_bg(paint: Background) -> Self {
        Self {
            paint,
            clip: ClipMode::Rect,
        }
    }

    /// Painted background plus rounded-corner stencil clip. Children
    /// are stencil-clipped to the paint's rounded corners — the
    /// stencil render path lights up. If `paint.radius` is zero the
    /// installer downgrades to scissor clip.
    pub const fn clip_rounded_with_bg(paint: Background) -> Self {
        Self {
            paint,
            clip: ClipMode::Rounded,
        }
    }
}

/// Sugar: `.background(Background { … })` keeps working — paint-only with
/// no clip is still expressible without typing the wrapper.
impl From<Background> for Surface {
    fn from(paint: Background) -> Self {
        Self {
            paint,
            clip: ClipMode::None,
        }
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
    /// Default surface for container widgets (`Panel`, `Grid`) when the
    /// call site didn't pass `.background(...)`. `None` = containers paint
    /// nothing and don't clip. Setting `Some(...)` lights up every
    /// unstyled container at once — useful for prototyping (set a thin
    /// stroke and every panel boundary becomes visible) or for shipping a
    /// design-system default that clips children to a rounded card shape.
    pub panel: Option<Surface>,
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

/// Paint settings for one widget state — the same shape that Button
/// (`normal`/`hovered`/`pressed`/`disabled`) and TextEdit
/// (`normal`/`focused`/`disabled`) both reach for. `Some(x)`
/// overrides; `None` inherits the framework default for that field.
/// `background = None` inherits [`Background::default`] (paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`Theme::text`], so an app changing `theme.text.color`
/// moves every label that didn't override it.
///
/// Per-theme `pick(state)` returns `&WidgetLook`; widgets call
/// [`Self::animate`] to interpolate the look's components and get an
/// [`AnimatedLook`] ready to render with.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Resolved + per-frame animated values for a [`WidgetLook`]. Built
/// by [`WidgetLook::animate`]; widgets read flat fields and call
/// [`Self::background`] to assemble the paint-ready chrome.
#[derive(Clone, Copy, Debug)]
pub struct AnimatedLook {
    pub fill: Color,
    /// `width = 0` (or transparent color) means "no stroke" — handled
    /// by [`Self::background`] when assembling the paint-ready chrome.
    pub stroke: Stroke,
    pub radius: Corners,
    pub text_color: Color,
    pub font_size_px: f32,
    pub line_height_mult: f32,
}

impl AnimatedLook {
    /// Assemble a paint-ready [`Background`]. Drops the stroke when
    /// the animated width has collapsed below epsilon or the color is
    /// transparent — keeps "stroked → no-stroke" transitions from
    /// leaking a phantom hairline.
    pub fn background(&self) -> Background {
        Background {
            fill: self.fill,
            stroke: (self.stroke.width > f32::EPSILON && !self.stroke.color.approx_transparent())
                .then_some(self.stroke),
            radius: self.radius,
        }
    }

    pub fn line_height_px(&self) -> f32 {
        self.font_size_px * self.line_height_mult
    }
}

impl WidgetLook {
    /// Slots reserved by [`Self::animate`] (4 of them: fill, stroke
    /// color, stroke width, text color). Widgets that mix in
    /// additional animations on the same `WidgetId` start their own
    /// slots at `WIDGETLOOK_SLOTS` to avoid collision.
    pub const WIDGETLOOK_SLOTS: u8 = 4;

    /// Resolve the look's components to flat values, animating each
    /// non-trivial component toward the target via `spec`. Pass
    /// `spec = None` to snap (the call shape stays the same — caller
    /// doesn't fork on whether motion is configured).
    ///
    /// `fallback_text` provides defaults for `self.text == None`
    /// (typically `ui.theme.text.clone()` from the caller; clone
    /// because `&ui.theme.text` would conflict with the `&mut Ui`).
    pub fn animate(
        &self,
        ui: &mut Ui,
        id: WidgetId,
        fallback_text: &TextStyle,
        spec: Option<AnimSpec>,
    ) -> AnimatedLook {
        let bg = self.background.unwrap_or_default();
        let stroke = bg.stroke.unwrap_or(Stroke {
            width: 0.0,
            color: Color::TRANSPARENT,
        });
        let text = self.text.as_ref().unwrap_or(fallback_text);
        AnimatedLook {
            fill: ui.animate(id, AnimSlot(0), bg.fill, spec),
            stroke: Stroke {
                color: ui.animate(id, AnimSlot(1), stroke.color, spec),
                width: ui.animate(id, AnimSlot(2), stroke.width, spec),
            },
            radius: bg.radius,
            text_color: ui.animate(id, AnimSlot(3), text.color, spec),
            font_size_px: text.font_size_px,
            line_height_mult: text.line_height_mult,
        }
    }
}

/// Three-state TextEdit theme. The leaf type ([`WidgetLook`]) lives
/// next to it; widget reads `theme.{normal,focused,disabled}` based
/// on `Element::disabled` and focus. Use [`Self::pick`] to select.
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
    pub normal: WidgetLook,
    pub focused: WidgetLook,
    pub disabled: WidgetLook,
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
    /// Spec applied to fill/stroke/text transitions between states.
    /// Default `None` — animation is opt-in (matches `ButtonTheme`).
    /// Round-trips through serde so theme files configure motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

impl TextEditTheme {
    /// Pick the visual state for `state.disabled` + `focused`.
    /// Disabled wins over focused; otherwise normal. `state.disabled`
    /// is the cascaded ancestor-or-self flag — caller can merge
    /// `state.disabled |= element.disabled` for lag-free response to
    /// its own self-toggle (mirrors Button's pattern).
    pub fn pick(&self, state: ResponseState, focused: bool) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if focused {
            &self.focused
        } else {
            &self.normal
        }
    }
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
            normal: WidgetLook {
                background: Some(normal_bg),
                text: None,
            },
            focused: WidgetLook {
                background: Some(focused_bg),
                text: None,
            },
            disabled: WidgetLook {
                background: Some(disabled_bg),
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            placeholder: palette::TEXT_MUTED,
            caret: palette::TEXT,
            caret_width: 1.5,
            selection,
            padding: Spacing::xy(8.0, 6.0),
            margin: Spacing::ZERO,
            anim: None,
        }
    }
}

/// Four-state button theme. The leaf type ([`WidgetLook`]) is shared
/// with `TextEditTheme`; widget reads `theme.{normal,hovered,pressed,
/// disabled}` based on the live response state and `Element::disabled`.
///
/// `padding`/`margin` apply when the user didn't call `.padding(...)`
/// / `.margin(...)` on the builder. The "user didn't override" check
/// is `element.padding == Spacing::ZERO` — so if you want a button
/// with no padding while the theme has padding, set a custom theme
/// rather than passing zero.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ButtonTheme {
    pub normal: WidgetLook,
    pub hovered: WidgetLook,
    pub pressed: WidgetLook,
    pub disabled: WidgetLook,
    /// Default padding inside the button (around the label).
    /// Applied at `show()` time when the builder hasn't set padding.
    pub padding: Spacing,
    /// Default margin around the button.
    pub margin: Spacing,
    /// Spec applied to fill/stroke/text transitions between states.
    /// Default `None` — animation is opt-in. Themes that want motion
    /// set this to `Some(AnimSpec::FAST)`, `Some(AnimSpec::SPRING)`,
    /// or any custom spec. Round-trips through serde so theme files
    /// can configure motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
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
            normal: WidgetLook {
                background: bg(palette::ELEM_HOVER),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(palette::ELEM_ACTIVE),
                text: None,
            },
            pressed: WidgetLook {
                background: Some(pressed_bg),
                text: None,
            },
            disabled: WidgetLook {
                background: bg(palette::ELEM),
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            padding: Spacing::xy(12.0, 6.0),
            margin: Spacing::ZERO,
            anim: None,
        }
    }
}

impl ButtonTheme {
    /// Pick the visual state for `state`. Disabled wins over
    /// hover/press; pressed wins over hover; otherwise normal.
    /// `state.disabled` is the cascaded ancestor-or-self flag — if
    /// the caller wants lag-free response to its own self-toggle,
    /// merge `state.disabled |= element.disabled` before calling.
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if state.pressed {
            &self.pressed
        } else if state.hovered {
            &self.hovered
        } else {
            &self.normal
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
