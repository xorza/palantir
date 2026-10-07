//! What a button wears in each of its four interaction states.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widget_core::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;

/// Four-state button theme: a [`StatefulLook`] (`active` = pressed) plus container knobs, picked via [`ThemeSlot::look`](crate::widget::ThemeSlot::look). `padding`/`margin` apply when the builder set none.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ButtonTheme {
    /// The four per-state looks; `flatten` keeps theme files flat (`[button.normal]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ButtonTheme {
    /// The standard button recipe over `p`, resting at `element_mid`; disabled keeps `element` with `text_disabled`. `text: None` on active states inherits `Theme::text`.
    pub fn from_palette(p: &Palette) -> Self {
        let bg = |fill: RgbaF32| {
            Background::rounded(fill, Corners::all(4.0))
                .with_border(Stroke::new(p.border_soft(), 1.0))
        };
        // Pressed = hovered fill + focused stroke (the palette has no further fill tier).
        let pressed_bg = Background::rounded(p.element_strong, Corners::all(4.0))
            .with_border(Stroke::new(p.border_focused, 1.0));
        Self {
            looks: StatefulLook {
                normal: WidgetLook {
                    background: bg(p.element_mid),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    background: bg(p.element_strong),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    background: pressed_bg,
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    background: bg(p.element),
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(12.0, 6.0),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }

    /// Visit every text slot this theme owns (drives `Theme::scale_text`); destructured so a new field fails to compile here.
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self { looks, defaults: _ } = self;
        looks.for_each_text(f);
    }

    /// Flat "menu-trigger" preset for menu-bar `Button`s: transparent at rest, hover-only background, no border or shadow. Deliberately a recipe, not a [`Theme`] slot, since no widget resolves against it; hand it to [`Button::style`].
    ///
    /// [`Theme`]: crate::Theme
    /// [`Button::style`]: crate::Button::style
    pub fn menu_button(p: &Palette) -> Self {
        let flat = |fill: Brush| WidgetLook {
            background: Background::rounded(fill, Corners::all(4.0)),
            text: TextStyleOverrides::NONE,
        };
        Self {
            looks: StatefulLook {
                normal: flat(Brush::TRANSPARENT),
                hovered: flat(p.element_mid.into()),
                active: flat(p.element_strong.into()),
                disabled: flat(Brush::TRANSPARENT),
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 4.0),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl ThemeSlot for ButtonTheme {
    type Pick = ();

    /// `active` = pressed. Disabled wins over hover and press, pressed over hover; `response.disabled` already carries the node's own flag.
    #[inline(always)]
    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.looks.pick(response, response.pressed())
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
