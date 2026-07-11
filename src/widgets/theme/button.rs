use crate::animation::AnimSpec;
use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::{WidgetLook, pick_4};

/// Four-state button theme. The leaf type ([`WidgetLook`]) is shared
/// with `TextEditTheme`; widget reads `theme.{normal,hovered,pressed,
/// disabled}` based on the live response state and `Element::disabled`.
///
/// `padding`/`margin` apply when the user didn't call `.padding(...)`
/// / `.margin(...)` on the builder. The "user didn't override" check
/// is `element.padding == Spacing::ZERO` — so if you want a button
/// with no padding while the theme has padding, set a custom theme
/// rather than passing zero.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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

impl ButtonTheme {
    /// Visit every `TextStyle` this theme owns — drives `Theme::set_text_scale`.
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.normal.for_each_text(f);
        self.hovered.for_each_text(f);
        self.pressed.for_each_text(f);
        self.disabled.for_each_text(f);
    }
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
        let edge = m.with_alpha(0.18);
        let bg = |fill: Color| {
            Some(Background::rounded(fill, Corners::all(4.0)).with_stroke(Stroke::solid(edge, 1.0)))
        };
        // Pressed = hovered fill + focused stroke (palette has no further fill tier).
        let pressed_bg = Background::rounded(palette::ELEM_ACTIVE, Corners::all(4.0))
            .with_stroke(Stroke::solid(palette::BORDER_FOCUSED, 1.0));
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
    /// Flat "menu-trigger" preset. Use for `Button`s that act as
    /// menu-bar entries (File / Edit / View etc.) — transparent at
    /// rest, hover-only background, no border or shadow, tighter
    /// padding than the default chunky `Button`. The trigger reads as
    /// plain text until the pointer is over it; matches the
    /// conventional menu-bar look (Figma / VS Code / macOS).
    /// Distinct from a popup-row `MenuItem`, which lives inside a
    /// `ContextMenu` and is themed via `theme.context_menu.item`.
    pub fn menu_button() -> Self {
        let flat = |fill: Brush| WidgetLook {
            background: Some(Background::rounded(fill, Corners::all(4.0))),
            text: None,
        };
        Self {
            normal: flat(Brush::TRANSPARENT),
            hovered: flat(palette::ELEM_HOVER.into()),
            pressed: flat(palette::ELEM_ACTIVE.into()),
            disabled: flat(Brush::TRANSPARENT),
            padding: Spacing::xy(8.0, 4.0),
            margin: Spacing::ZERO,
            anim: None,
        }
    }

    /// Pick the visual state for `state`. Disabled wins over
    /// hover/press; pressed wins over hover; otherwise normal.
    /// `state.disabled` is the cascaded ancestor-or-self flag — if
    /// the caller wants lag-free response to its own self-toggle,
    /// merge `state.disabled |= element.disabled` before calling.
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        pick_4(
            state,
            &self.normal,
            &self.hovered,
            &self.pressed,
            &self.disabled,
        )
    }
}
