//! What a tab strip wears: chip look packs, the selection cap, and the band.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::palette::Palette;
use crate::theme::tab_selection::TabSelection;
use crate::theme::tab_strip_focus::TabStripFocus;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Visuals for [`crate::TabStrip`], [`crate::TabbedView`] and every
/// [`crate::DockView`] pane. One four-state look pack per selected state,
/// picked by [`StatefulLook::pick`]. The selected chip wears a top cap:
/// [`Self::cap_focused`] while the strip holds focus, else [`Self::cap_unfocused`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TabsTheme {
    /// Look pack for the selected chip; its fill defaults to the window ground.
    pub selected: StatefulLook,
    /// Look pack for every other chip.
    pub unselected: StatefulLook,
    /// Selection cap on the focused strip.
    pub cap_focused: RgbaF32,
    /// Selection cap on a strip that does not hold focus.
    pub cap_unfocused: RgbaF32,
    /// Cap breadth in logical px; the selected chip lifts its top inset by the same amount.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub cap_thickness: f32,
    /// The band behind the chips; [`Chrome::NONE`] by default.
    pub strip: Chrome,
    /// Inset between the band's edges and the chips.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub strip_padding: Spacing,
    /// Gutter between two chips.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
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
    /// Chip width ceiling, so a long title ellipsizes.
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
    /// The insertion mark a chip drag shows at the slot it would drop into.
    pub caret: RgbaF32,
    /// Breadth of the insertion mark in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub caret_width: f32,
    /// Gutter between a chip's icon, label, badge and close button.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub label_gap: f32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl TabsTheme {
    /// Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            selected,
            unselected,
            close,
            cap_focused: _,
            cap_unfocused: _,
            cap_thickness: _,
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
            caret: _,
            caret_width: _,
            label_gap: _,
            defaults: _,
        } = self;
        selected.for_each_text(f);
        unselected.for_each_text(f);
        close.for_each_text(f);
    }

    /// The cap color a strip paints under its selected chip.
    pub const fn cap(&self, focus: TabStripFocus) -> RgbaF32 {
        match focus {
            TabStripFocus::Focused => self.cap_focused,
            TabStripFocus::Unfocused => self.cap_unfocused,
        }
    }

    /// Chips rounded at the top only.
    pub fn from_palette(p: &Palette) -> Self {
        let radius = 4.0;
        let top = Corners::top(radius);
        let inactive_text = TextStyleOverrides::NONE.with_color(p.text_muted);
        let disabled_text = TextStyleOverrides::NONE.with_color(p.text_disabled);
        let chip = |fill: RgbaF32, text: TextStyleOverrides| WidgetLook {
            chrome: Chrome::rounded(fill, top),
            text,
        };
        Self {
            selected: StatefulLook {
                normal: chip(p.window_bg, TextStyleOverrides::NONE),
                hovered: chip(p.window_bg, TextStyleOverrides::NONE),
                active: chip(p.window_bg, TextStyleOverrides::NONE),
                disabled: chip(p.window_bg, disabled_text),
            },
            unselected: StatefulLook {
                normal: chip(p.element_mid, inactive_text),
                hovered: chip(p.element_strong, inactive_text),
                active: chip(p.element_strong, TextStyleOverrides::NONE),
                disabled: chip(p.element, disabled_text),
            },
            cap_focused: p.accent,
            cap_unfocused: p.element_strong,
            cap_thickness: 2.0,
            strip: Chrome::NONE,
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
                    chrome: Chrome::NONE,
                    text: inactive_text,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(p.element_strong, Corners::all(3.0)),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(p.element_strong, Corners::all(3.0))
                        .with_border(Stroke::new(p.border_focused, 1.0)),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::NONE,
                    text: disabled_text,
                },
            },
            close_size: 16.0,
            badge: p.accent,
            badge_size: 3.5,
            caret: p.accent,
            caret_width: 3.0,
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
    type Pick = TabSelection;

    /// The selected or unselected pack, then its state (`active` = pressed).
    fn look(&self, response: &ResponseState, selection: TabSelection) -> &WidgetLook {
        let pack = match selection {
            TabSelection::Selected => &self.selected,
            TabSelection::Unselected => &self.unselected,
        };
        pack.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
