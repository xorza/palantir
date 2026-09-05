//! What a button wears in each of its four interaction states.

use crate::input::response::response_state::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::widget_look::theme_slot::{SlotDefaults, ThemeSlot};

/// Four-state button theme: a [`StatefulLook`] (`active` = pressed)
/// plus the container knobs. The widget picks a look from the live
/// response state and `Node::disabled` via [`Self::pick`].
///
/// `padding`/`margin` apply when the user didn't call `.padding(...)`
/// / `.margin(...)` on the builder. Explicit zero spacing overrides
/// the theme like any other value.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ButtonTheme {
    /// The four per-state looks. `flatten` keeps theme files flat
    /// (`[button.normal]`, not `[button.looks.normal]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ButtonTheme {
    /// The standard button recipe over `p`: clickable-surface family
    /// `elem` / `elem_mid` / `elem_strong`, resting one rung up at
    /// `elem_mid`. Disabled keeps the `elem` fill but swaps
    /// text to `text_disabled`. `text: None` on active states means
    /// "inherit `Theme::text`" — bumping `theme.text.color` recolors
    /// active button labels. The historical 4 px radius is retained.
    pub fn from_palette(p: &Palette) -> Self {
        let bg = |fill: RgbaF32| {
            Background::rounded(fill, Corners::all(4.0))
                .with_stroke(Stroke::solid(p.border_soft(), 1.0))
        };
        // Pressed = hovered fill + focused stroke (the palette has no further fill tier).
        let pressed_bg = Background::rounded(p.elem_strong, Corners::all(4.0))
            .with_stroke(Stroke::solid(p.border_focused, 1.0));
        Self {
            looks: StatefulLook {
                normal: WidgetLook {
                    background: bg(p.elem_mid),
                    text: None,
                },
                hovered: WidgetLook {
                    background: bg(p.elem_strong),
                    text: None,
                },
                active: WidgetLook {
                    background: pressed_bg,
                    text: None,
                },
                disabled: WidgetLook {
                    background: bg(p.elem),
                    text: Some(TextStyle::default().with_color(p.text_disabled)),
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 6.0),
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }

    /// Visit every `TextStyle` this theme owns — drives `Theme::scale_text`.
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self { looks, defaults: _ } = self;
        looks.for_each_text(f);
    }

    /// Flat "menu-trigger" preset. Use for `Button`s that act as
    /// menu-bar entries (File / Edit / View etc.) — transparent at
    /// rest, hover-only background, no border or shadow, tighter
    /// padding than the default chunky `Button`. The trigger reads as
    /// plain text until the pointer is over it; matches the
    /// conventional menu-bar look (Figma / VS Code / macOS).
    /// Distinct from a popup-row `MenuItem`, which lives inside a
    /// `ContextMenu` and is themed via `theme.context_menu.item`.
    ///
    /// Deliberately a recipe rather than a [`Theme`] slot: no widget in
    /// the crate resolves against a menu-bar style, so a slot would be a
    /// theme field, a serde shape, and a text-walk arm that nothing
    /// reads. An app with a menu bar builds one from its own palette and
    /// hands it to [`Button::style`].
    ///
    /// [`Theme`]: crate::Theme
    /// [`Button::style`]: crate::Button::style
    pub fn menu_button(p: &Palette) -> Self {
        let flat = |fill: Brush| WidgetLook {
            background: Background::rounded(fill, Corners::all(4.0)),
            text: None,
        };
        Self {
            looks: StatefulLook {
                normal: flat(Brush::TRANSPARENT),
                hovered: flat(p.elem_mid.into()),
                active: flat(p.elem_strong.into()),
                disabled: flat(Brush::TRANSPARENT),
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 4.0),
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }

    /// Pick the visual state for `state`: `active` = pressed.
    /// Disabled wins over hover/press; pressed wins over hover;
    /// otherwise normal. `state.disabled` already carries the node's own
    /// flag — [`Widget::response`](crate::Widget) merges it, so a button
    /// disabled this frame paints disabled without waiting for the
    /// cascade.
    #[inline(always)]
    pub fn pick(&self, state: &ResponseState) -> &WidgetLook {
        self.looks.pick(state, state.pressed())
    }
}

impl ThemeSlot for ButtonTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.pick(response)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
