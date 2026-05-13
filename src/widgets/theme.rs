use crate::animation::{AnimSlot, AnimSpec};
use crate::input::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::text::FontFamily;
use crate::ui::Ui;
use palantir_anim_derive::Animatable;

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
    pub context_menu: ContextMenuTheme,
    pub tooltip: TooltipTheme,
    pub text: TextStyle,
    /// Window/swapchain clear color. Hosts pass to `WgpuBackend::submit`.
    pub window_clear: Color,
    /// Default chrome paint for container widgets (`Panel`, `Grid`,
    /// `Popup`) that didn't call [`Configure::background`]. `None`
    /// leaves containers unpainted by default. Setting `Some(...)`
    /// lights up every unstyled container at once — useful for
    /// prototyping or shipping a design-system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_background: Option<Background>,
    /// Default clip mode for container widgets that didn't call
    /// [`Configure::clip_rect`] / [`Configure::clip_rounded`]. Pairs
    /// with [`Self::panel_background`]; the chrome's `radius` supplies
    /// the rounded-clip mask geometry.
    #[serde(default, skip_serializing_if = "is_clip_none")]
    pub panel_clip: ClipMode,
}

#[inline]
fn is_clip_none(c: &ClipMode) -> bool {
    matches!(c, ClipMode::None)
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            scrollbar: ScrollbarTheme::default(),
            text_edit: TextEditTheme::default(),
            context_menu: ContextMenuTheme::default(),
            tooltip: TooltipTheme::default(),
            text: TextStyle::default(),
            window_clear: palette::TERMINAL_BG,
            panel_background: None,
            panel_clip: ClipMode::None,
        }
    }
}

/// Default text-rendering inputs grouped together so apps can swap the
/// whole "text look" with one assignment, and so future axes (font
/// family, weight, italic, letter-spacing) extend a single struct
/// rather than scattering across [`Theme`].
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
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WidgetLook {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Resolved + per-frame animated values for a [`WidgetLook`]. Built
/// by [`WidgetLook::animate`]. Widgets read `background` and `text`
/// directly; both fields are already-animated.
///
/// `text.color` is the animated color; `text.font_size_px` and
/// `text.line_height_mult` are snap-carried from the picked
/// `WidgetLook` (or the fallback) — see `TextStyle`'s
/// `#[animate(snap)]` markings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Animatable)]
pub struct AnimatedLook {
    pub background: Background,
    pub text: TextStyle,
}

impl AnimatedLook {
    /// Convenience: `text.line_height_for(text.font_size_px)`. Widgets
    /// rendering `ShapeRecord::Text` need this paired with `font_size_px`
    /// for the shaper.
    pub fn line_height_px(&self) -> f32 {
        self.text.line_height_for(self.text.font_size_px)
    }
}

impl WidgetLook {
    /// Slot [`Self::animate`] reserves on the widget's id. One row
    /// per widget animates the whole resolved look (background + text)
    /// — halves `Ui::animate` call traffic compared to per-component
    /// slots.
    const SLOT_LOOK: AnimSlot = AnimSlot("look");

    /// Resolve the look to flat animated values. `Background` (fill +
    /// stroke) animates as one slot; `TextStyle` (color animated,
    /// font/leading snapped) as another. Pass `spec = None` to snap
    /// everything; call shape stays the same so callers don't fork
    /// on motion.
    ///
    /// `fallback_text` is used when `self.text == None` — pass
    /// `ui.theme.text` (TextStyle is `Copy`).
    pub fn animate(
        &self,
        ui: &mut Ui,
        id: WidgetId,
        fallback_text: TextStyle,
        spec: Option<AnimSpec>,
    ) -> AnimatedLook {
        let target = AnimatedLook {
            background: self.background.unwrap_or_default(),
            text: self.text.unwrap_or(fallback_text),
        };
        ui.animate(id, Self::SLOT_LOOK, target, spec)
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
    /// Pick the visual state. Disabled wins over focused; otherwise
    /// normal. `state.disabled` is the cascaded ancestor-or-self flag
    /// — caller can merge `state.disabled |= element.disabled` for
    /// lag-free response to its own self-toggle (mirrors Button).
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if state.focused {
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
            fill: palette::ELEM_HOVER.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius,
            shadow: Shadow::NONE,
        };
        let focused_bg = Background {
            fill: palette::ELEM_HOVER.into(),
            stroke: Stroke::solid(palette::BORDER_FOCUSED, 1.5),
            radius,
            shadow: Shadow::NONE,
        };
        let disabled_bg = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius,
            shadow: Shadow::NONE,
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
                fill: fill.into(),
                stroke: Stroke::solid(edge, 1.0),
                radius: Corners::all(4.0),
                shadow: Shadow::NONE,
            })
        };
        // Pressed = hovered fill + focused stroke (palette has no further fill tier).
        let pressed_bg = Background {
            fill: palette::ELEM_ACTIVE.into(),
            stroke: Stroke::solid(palette::BORDER_FOCUSED, 1.0),
            radius: Corners::all(4.0),
            shadow: Shadow::NONE,
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

/// Visuals for [`crate::widgets::popup::Popup`]-hosted context menus.
/// `panel` paints the surrounding container chrome (fill + stroke +
/// radius); `item` drives [`MenuItem`] rows. `min_width` is the
/// floor for the menu's container Sizing on the main axis so single-
/// character labels don't paint as a one-glyph-wide pill.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextMenuTheme {
    /// Panel chrome behind the items. Container's `padding` carves the
    /// gutter between chrome and rows.
    pub panel: Background,
    /// Padding inside the container, around the column of items.
    pub padding: Spacing,
    /// Floor for the menu's container width.
    pub min_width: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin horizontal divider between groups (for `MenuItem::separator`).
    pub separator: Color,
    /// Optional motion spec on `MenuItem` hover/press transitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

/// Three-state row look for [`crate::widgets::context_menu::MenuItem`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuItemTheme {
    pub normal: WidgetLook,
    pub hovered: WidgetLook,
    pub disabled: WidgetLook,
    /// Color for the right-aligned shortcut hint (e.g. "⌘C"). Pulled
    /// off the row label color so the hint reads muted.
    pub shortcut: Color,
    /// Padding inside one row.
    pub padding: Spacing,
}

impl MenuItemTheme {
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if state.hovered {
            &self.hovered
        } else {
            &self.normal
        }
    }
}

impl Default for ContextMenuTheme {
    fn default() -> Self {
        let m = palette::TEXT_MUTED;
        let edge = Color::linear_rgba(m.r, m.g, m.b, 0.22);
        let panel = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius: Corners::all(6.0),
            shadow: Shadow::NONE,
        };
        let separator = Color::linear_rgba(m.r, m.g, m.b, 0.18);
        Self {
            panel,
            padding: Spacing::all(4.0),
            min_width: 160.0,
            item: MenuItemTheme::default(),
            separator,
            anim: None,
        }
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        // Rows are transparent at rest; hover paints accent-tinted fill.
        let hover_bg = Background {
            fill: palette::ELEM_ACTIVE.into(),
            stroke: Stroke::ZERO,
            radius: Corners::all(4.0),
            shadow: Shadow::NONE,
        };
        Self {
            normal: WidgetLook::default(),
            hovered: WidgetLook {
                background: Some(hover_bg),
                text: None,
            },
            disabled: WidgetLook {
                background: None,
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            shortcut: palette::TEXT_MUTED,
            padding: Spacing::xy(10.0, 6.0),
        }
    }
}

/// Visuals + timing for [`crate::widgets::tooltip::Tooltip`]. Bubbles
/// paint into `Layer::Tooltip` after the pointer has hovered a trigger
/// for `delay` seconds; the `warmup` window keeps subsequent tooltips
/// instant for a short period after one was dismissed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TooltipTheme {
    /// Bubble chrome (fill + stroke + radius + optional shadow).
    pub panel: Background,
    /// Text inside the bubble.
    pub text: TextStyle,
    /// Padding between chrome and the text.
    pub padding: Spacing,
    /// Cap on the bubble's outer size. Width gates wrap; height is
    /// usually `INF` so tall tooltips just keep growing. Builder
    /// callers override via `.max_size(...)` (`Configure`).
    pub max_size: Size,
    /// Seconds the pointer must rest on the trigger before the bubble
    /// shows (cold start).
    pub delay: f32,
    /// Seconds after a tooltip is dismissed during which the next
    /// tooltip appears instantly (warmup). Set to 0 to disable.
    pub warmup: f32,
    /// Gap in logical px between trigger rect and bubble.
    pub gap: f32,
}

impl Default for TooltipTheme {
    fn default() -> Self {
        let m = palette::TEXT_MUTED;
        let edge = Color::linear_rgba(m.r, m.g, m.b, 0.22);
        let panel = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius: Corners::all(4.0),
            shadow: Shadow {
                color: Color::linear_rgba(0.0, 0.0, 0.0, 0.6),
                offset: glam::Vec2::new(2.0, 2.0),
                blur: 5.0,
                spread: 0.0,
                inset: false,
            },
        };
        Self {
            panel,
            text: TextStyle::default().with_font_size(13.0),
            padding: Spacing::xy(6.0, 4.0),
            max_size: Size::new(280.0, f32::INFINITY),
            delay: 0.5,
            warmup: 1.0,
            gap: 6.0,
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

    /// `WidgetLook` round-trips through TOML for both variants
    /// (background present / absent, text override / inherit).
    /// Pinned because theme files are a public surface.
    #[test]
    fn widget_look_serde_roundtrip() {
        let cases = [
            WidgetLook::default(),
            WidgetLook {
                background: Some(Background {
                    fill: Color::hex(0x336699).into(),
                    stroke: Stroke::solid(Color::hex(0xffffff), 1.5),
                    radius: Corners::all(6.0),
                    shadow: Shadow::NONE,
                }),
                text: Some(TextStyle::default().with_font_size(20.0)),
            },
        ];
        for look in cases {
            let s = toml::to_string_pretty(&look).expect("serialize");
            let back: WidgetLook = toml::from_str(&s).expect("parse");
            assert_eq!(look, back);
        }
    }

    /// `ButtonTheme::pick` precedence: disabled > pressed > hovered >
    /// normal. Table-driven sweep — every state combination resolves
    /// to the right slot, so reordering the if-cascade silently is
    /// caught.
    #[test]
    fn button_theme_pick_precedence() {
        let theme = ButtonTheme::default();
        let s = |hovered, pressed, disabled| ResponseState {
            hovered,
            pressed,
            disabled,
            ..ResponseState::default()
        };
        let cases: &[(ResponseState, &WidgetLook, &str)] = &[
            (s(false, false, false), &theme.normal, "normal"),
            (s(true, false, false), &theme.hovered, "hovered"),
            (s(true, true, false), &theme.pressed, "pressed > hovered"),
            (s(false, false, true), &theme.disabled, "disabled (idle)"),
            (s(true, true, true), &theme.disabled, "disabled wins all"),
        ];
        for (state, expected, label) in cases {
            assert!(
                std::ptr::eq(theme.pick(*state), *expected),
                "{label}: pick should return the matching slot",
            );
        }
    }

    /// `TextEditTheme::pick`: disabled > focused > normal. Reads
    /// `state.focused` from `ResponseState` (no separate parameter
    /// since `focused` is in-state now).
    #[test]
    fn text_edit_theme_pick_precedence() {
        let theme = TextEditTheme::default();
        let s = |focused, disabled| ResponseState {
            disabled,
            focused,
            ..ResponseState::default()
        };
        let cases: &[(ResponseState, &WidgetLook, &str)] = &[
            (s(false, false), &theme.normal, "normal"),
            (s(true, false), &theme.focused, "focused"),
            (s(false, true), &theme.disabled, "disabled (unfocused)"),
            (s(true, true), &theme.disabled, "disabled wins focus"),
        ];
        for (state, expected, label) in cases {
            assert!(
                std::ptr::eq(theme.pick(*state), *expected),
                "{label}: pick should return the matching slot",
            );
        }
    }

    /// Pins tooltip defaults: delay/warmup/max-width are user-facing
    /// timings, regressing them is a visible UX change.
    #[test]
    fn tooltip_theme_defaults() {
        let t = TooltipTheme::default();
        assert!((t.delay - 0.5).abs() < 1e-6);
        assert!((t.warmup - 1.0).abs() < 1e-6);
        assert!((t.max_size.w - 280.0).abs() < 1e-6);
        assert!(t.max_size.h.is_infinite());
        assert!((t.gap - 6.0).abs() < 1e-6);
    }

    /// `AnimatedLook::line_height_px` delegates to `TextStyle`'s
    /// formula (`font_size_px * line_height_mult`). Pinned because the
    /// shaper depends on it staying in sync with widget render code.
    #[test]
    fn animated_look_line_height_px_delegates_to_text_style() {
        let look = AnimatedLook {
            background: Background::default(),
            text: TextStyle {
                font_size_px: 16.0,
                color: Color::TRANSPARENT,
                line_height_mult: 1.5,
                family: FontFamily::Sans,
            },
        };
        assert!((look.line_height_px() - 24.0).abs() < 1e-6);
    }
}
