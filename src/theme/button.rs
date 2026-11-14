//! What a button wears in each of its four interaction states.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::selection::Selection;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// How far [`ButtonTheme::accent`]'s fill moves away from its label's ink under the pointer.
pub(crate) const ACCENT_HOVER: f32 = 0.12;
/// How far it moves pressed.
pub(crate) const ACCENT_PRESS: f32 = 0.24;

/// Four-state button theme: a [`StatefulLook`] (`active` = pressed) for a button at rest and one
/// for a [selected](crate::Button::selected) button, plus container knobs, picked via
/// [`ThemeSlot::look`]. `padding`/`margin` apply when the builder set none.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct ButtonTheme {
    /// The four per-state looks of a button that is not selected (`[button.unselected.normal]`).
    pub unselected: StatefulLook,
    /// The four per-state looks of a button that shows its on state (`[button.selected.normal]`).
    pub selected: StatefulLook,
    /// Spacing and transition spec — see [`SlotDefaults`].
    pub defaults: SlotDefaults,
}

impl ButtonTheme {
    /// The standard button recipe over `p` and `m`, resting at `element_mid`; disabled keeps `element` with `text_disabled`. `text: None` on active states inherits `Theme::text`. Selected, it wears the pressed look in every live state, as a held button reads. A state change runs under the metrics' `transition`.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let corners = Corners::all(m.radius.control);
        let chrome = |fill: RgbaF32| {
            Chrome::rounded(fill, corners)
                .with_border(Border::new(p.border_soft(), m.stroke.hairline))
        };
        // Pressed = hovered fill + focused stroke (the palette has no further fill tier).
        let pressed_chrome = Chrome::rounded(p.element_strong, corners)
            .with_border(Border::new(p.border_focused, m.stroke.hairline));
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
                transition: m.transition,
                press: None,
            },
        }
    }

    /// The filled call to action: the accent under `on_accent` ink, as WinUI's accent button and
    /// Material's filled button carry a dialog's confirming choice. Hovered and pressed, the fill
    /// moves 12 % and 24 % of the way, in linear light, to white under dark `on_accent` ink and to
    /// black under light, away from the ink, so a state only raises the label's contrast:
    /// Material's state layer of `on_accent` would lower it, under 4.5:1 on the light palette's
    /// accent
    /// (<https://m3.material.io/foundations/interaction/states/state-layers>). Disabled and
    /// selected, it is [`Self::from_parts`]'s, as a disabled call to action no longer stands out.
    /// Refused: a danger button and a danger role on [`Palette`], which Fluent 2's appearances,
    /// Material 3 and Apple's HIG do not ship; an app builds its own on [`Self::from_parts`].
    pub fn accent(p: &Palette, m: &Metrics) -> Self {
        let stock = Self::from_parts(p, m);
        let corners = Corners::all(m.radius.control);
        let filled = |fill: RgbaF32| WidgetLook {
            chrome: Chrome::rounded(fill, corners)
                .with_border(Border::new(fill, m.stroke.hairline)),
            text: TextStyleOverrides::NONE.with_color(p.on_accent),
        };
        let away = if p.on_accent.luminance() < p.accent.luminance() {
            RgbaF32::WHITE
        } else {
            RgbaF32::BLACK
        };
        Self {
            unselected: StatefulLook {
                normal: filled(p.accent),
                hovered: filled(p.accent.mix(away, ACCENT_HOVER)),
                active: filled(p.accent.mix(away, ACCENT_PRESS)),
                disabled: stock.unselected.disabled,
            },
            selected: stock.selected,
            defaults: stock.defaults,
        }
    }

    /// The subtle button, as Fluent 2's subtle appearance: transparent at rest, hover-only fill, no border or shadow, inset as a menu row; disabled, its label takes `text_disabled`. Selected, it rests at the hovered fill, as an open menu's title does. [`MenuBarTheme`](crate::MenuBarTheme)'s titles start from it; hand it to [`Button::style`] for a flat button anywhere (<https://fluent2.microsoft.design/components/web/react/core/button/usage>).
    ///
    /// [`Button::style`]: crate::Button::style
    pub fn subtle(p: &Palette, m: &Metrics) -> Self {
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
                transition: m.transition,
                press: None,
            },
        }
    }
}

impl ThemeSlot for ButtonTheme {
    type Pick = Selection;

    /// The resting or the selected pack, then its state: `active` = pressed. Disabled wins over hover and press, pressed over hover; `response.disabled` already carries the node's own flag.
    #[inline(always)]
    fn look(&self, response: &Response, selection: Selection) -> &WidgetLook {
        selection
            .pack(&self.unselected, &self.selected)
            .pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
