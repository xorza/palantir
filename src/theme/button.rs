//! What a button wears in each of its four interaction states.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::selection::Selection;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Four-state button theme: a [`StatefulLook`] (`active` = pressed) for a button at rest and one
/// for a [selected](crate::Button::selected) button, plus container knobs, picked via
/// [`ThemeSlot::look`]. `padding`/`margin` apply when the builder set none.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ButtonTheme {
    /// The four per-state looks of a button that is not selected; `flatten` keeps theme files
    /// flat (`[button.normal]`).
    #[serde(flatten)]
    pub unselected: StatefulLook,
    /// The four per-state looks of a button that shows its on state (`[button.selected.normal]`).
    pub selected: StatefulLook,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ButtonTheme {
    /// The standard button recipe over `p` and `m`, resting at `element_mid`; disabled keeps `element` with `text_disabled`. `text: None` on active states inherits `Theme::text`. Selected, it wears the pressed look in every live state, as a held button reads. A state change runs under the metrics' `transition`.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let corners = Corners::all(m.radius.control);
        let chrome = |fill: RgbaF32| {
            Chrome::rounded(fill, corners)
                .with_border(Stroke::new(p.border_soft(), m.stroke.hairline))
        };
        // Pressed = hovered fill + focused stroke (the palette has no further fill tier).
        let pressed_chrome = Chrome::rounded(p.element_strong, corners)
            .with_border(Stroke::new(p.border_focused, m.stroke.hairline));
        let pressed = WidgetLook {
            chrome: pressed_chrome,
            text: TextStyleOverrides::NONE,
        };
        let disabled = WidgetLook {
            chrome: chrome(p.element),
            text: TextStyleOverrides::NONE.with_color(p.text_disabled),
        };
        Self {
            unselected: StatefulLook {
                normal: WidgetLook {
                    chrome: chrome(p.element_mid),
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: chrome(p.element_strong),
                    text: TextStyleOverrides::NONE,
                },
                active: pressed.clone(),
                disabled: disabled.clone(),
            },
            selected: StatefulLook {
                normal: pressed.clone(),
                hovered: pressed.clone(),
                active: pressed,
                disabled,
            },
            defaults: SlotDefaults {
                padding: Spacing::xy(m.space.md, m.space.xs),
                margin: Spacing::ZERO,
                animation: m.transition,
            },
        }
    }

    /// Visit every text slot this theme owns (drives `Theme::scale_text`); destructured so a new field fails to compile here.
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            unselected,
            selected,
            defaults: _,
        } = self;
        unselected.for_each_text(f);
        selected.for_each_text(f);
    }

    /// Flat "menu-trigger" preset: transparent at rest, hover-only fill, no border or shadow, inset as a menu row; disabled, its label takes `text_disabled`. Selected, it rests at the hovered fill, as an open menu's title does. [`MenuBarTheme`](crate::MenuBarTheme)'s titles start from it; hand it to [`Button::style`] for a flat button outside a menu bar.
    ///
    /// [`Button::style`]: crate::Button::style
    pub fn menu_button(p: &Palette, m: &Metrics) -> Self {
        let flat = |fill: Brush| WidgetLook {
            chrome: Chrome::rounded(fill, Corners::all(m.radius.control)),
            text: TextStyleOverrides::NONE,
        };
        let disabled = WidgetLook {
            text: TextStyleOverrides::NONE.with_color(p.text_disabled),
            ..flat(Brush::TRANSPARENT)
        };
        Self {
            unselected: StatefulLook {
                normal: flat(Brush::TRANSPARENT),
                hovered: flat(p.element_mid.into()),
                active: flat(p.element_strong.into()),
                disabled: disabled.clone(),
            },
            selected: StatefulLook {
                normal: flat(p.element_mid.into()),
                hovered: flat(p.element_mid.into()),
                active: flat(p.element_strong.into()),
                disabled,
            },
            defaults: SlotDefaults {
                padding: m.row_padding(),
                margin: Spacing::ZERO,
                animation: m.transition,
            },
        }
    }
}

impl ThemeSlot for ButtonTheme {
    type Pick = Selection;

    /// The resting or the selected pack, then its state: `active` = pressed. Disabled wins over hover and press, pressed over hover; `response.disabled` already carries the node's own flag.
    #[inline(always)]
    fn look(&self, response: &ResponseState, selection: Selection) -> &WidgetLook {
        let pack = match selection {
            Selection::Unselected => &self.unselected,
            Selection::Selected => &self.selected,
        };
        pack.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
