//! What a tab strip wears: chip look packs, the selection cap, and the band.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widget_core::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;

/// Visuals for [`crate::TabStrip`], [`crate::TabbedView`] and every
/// [`crate::DockView`] pane. One four-state look pack per selected state,
/// picked by [`StatefulLook::pick`]. The selected chip wears a top cap:
/// [`Self::accent`] while the strip holds focus, else [`Self::accent_idle`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TabsTheme {
    /// Look pack for the selected chip; its fill defaults to the window ground.
    pub active: StatefulLook,
    /// Look pack for every other chip.
    pub inactive: StatefulLook,
    /// Selection cap on the focused strip.
    pub accent: RgbaF32,
    /// Selection cap on a strip that does not hold focus.
    pub accent_idle: RgbaF32,
    /// Cap breadth in logical px; the selected chip lifts its top inset by the same amount.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub accent_thickness: f32,
    /// The band behind the chips; [`Background::NONE`] by default.
    pub strip: Background,
    /// Inset between the band's edges and the chips.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub strip_padding: Spacing,
    /// Gutter between two chips.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Hairline under the band, drawn only when [`Self::rule_thickness`] is set.
    pub rule: RgbaF32,
    /// Hairline breadth in logical px; `0.0` records no rule.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub rule_thickness: f32,
    /// Chip corner radius, top corners only.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub radius: f32,
    /// Inset between a chip's edges and its label, apart from [`SlotDefaults::padding`].
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub chip_padding: Spacing,
    /// Trailing inset replacing [`Self::chip_padding`]'s right one when a badge or close button follows.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub trailing_inset: f32,
    /// Chip width floor.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
    /// Chip width ceiling, so a long title ellipsises.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub max_width: f32,
    /// Look pack for the chip's close button.
    pub close: StatefulLook,
    /// Close button side in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub close_size: f32,
    /// Ink of the status dot.
    pub badge: RgbaF32,
    /// Status-dot diameter in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub badge_size: f32,
    /// Gutter between a chip's icon, label, badge and close button.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub label_gap: f32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl TabsTheme {
    /// Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            active,
            inactive,
            close,
            accent: _,
            accent_idle: _,
            accent_thickness: _,
            strip: _,
            strip_padding: _,
            gap: _,
            rule: _,
            rule_thickness: _,
            radius: _,
            chip_padding: _,
            trailing_inset: _,
            min_width: _,
            max_width: _,
            close_size: _,
            badge: _,
            badge_size: _,
            label_gap: _,
            defaults: _,
        } = self;
        active.for_each_text(f);
        inactive.for_each_text(f);
        close.for_each_text(f);
    }

    /// The cap colour a strip paints under its selected chip.
    pub const fn cap(&self, focused: bool) -> RgbaF32 {
        if focused {
            self.accent
        } else {
            self.accent_idle
        }
    }

    /// Chips rounded at the top only.
    pub fn from_palette(p: &Palette) -> Self {
        let radius = 4.0;
        let top = Corners::top(radius);
        let inactive_text = TextStyleOverrides::NONE.with_color(p.text_muted);
        let disabled_text = TextStyleOverrides::NONE.with_color(p.text_disabled);
        let chip = |fill: RgbaF32, text: TextStyleOverrides| WidgetLook {
            background: Background::rounded(fill, top),
            text,
        };
        Self {
            active: StatefulLook {
                normal: chip(p.window_background, TextStyleOverrides::NONE),
                hovered: chip(p.window_background, TextStyleOverrides::NONE),
                active: chip(p.window_background, TextStyleOverrides::NONE),
                disabled: chip(p.window_background, disabled_text),
            },
            inactive: StatefulLook {
                normal: chip(p.element_mid, inactive_text),
                hovered: chip(p.element_strong, inactive_text),
                active: chip(p.element_strong, TextStyleOverrides::NONE),
                disabled: chip(p.element, disabled_text),
            },
            accent: p.accent,
            accent_idle: p.element_strong,
            accent_thickness: 2.0,
            strip: Background::NONE,
            strip_padding: Spacing::new(6.0, 4.0, 6.0, 0.0),
            gap: 3.0,
            rule: p.border_soft(),
            rule_thickness: 0.0,
            radius,
            chip_padding: Spacing::new(10.0, 4.0, 10.0, 4.0),
            trailing_inset: 4.0,
            min_width: 48.0,
            max_width: 200.0,
            close: StatefulLook {
                normal: WidgetLook {
                    background: Background::NONE,
                    text: inactive_text,
                },
                hovered: WidgetLook {
                    background: Background::rounded(p.element_strong, Corners::all(3.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    background: Background::rounded(p.element_strong, Corners::all(3.0))
                        .with_border(Stroke::new(p.border_focused, 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: disabled_text,
                },
            },
            close_size: 16.0,
            badge: p.accent,
            badge_size: 3.5,
            label_gap: 6.0,
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl Default for TabsTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

impl ThemeSlot for TabsTheme {
    type Pick = bool;

    /// The selected or unselected pack, then its state (`active` = pressed).
    fn look(&self, response: &ResponseState, selected: bool) -> &WidgetLook {
        let pack = if selected {
            &self.active
        } else {
            &self.inactive
        };
        pack.pick(response, response.pressed())
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
