//! What a disclosure header wears, and how far its body is inset.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widget_core::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;

/// Visuals for [`crate::Expander`]; the header takes button-style four-state looks.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExpanderTheme {
    /// Header look.
    pub looks: StatefulLook,
    /// Triangle bounding box, corners included; keep it square unless the angles leave it unturned.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length2")]
    pub arrow_size: Vec2,
    /// Triangle corner radius; `0.0` is sharp; at most half the smaller side of [`Self::arrow_size`].
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub arrow_radius: f32,
    /// Arrow angle in radians while closed (default: points at the label).
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub arrow_closed_angle: f32,
    /// Arrow angle while open (default: upright). `0.0` and `-PI` give down-closed, up-open.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::angle")]
    pub arrow_open_angle: f32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    /// Gap between arrow and label.
    pub gap: f32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    /// Indent of the body.
    pub indent: f32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    /// Padding of the body.
    pub body_padding: Spacing,
    /// `animation` is `None` by default, so a reveal snaps.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ExpanderTheme {
    /// Destructured so a new field fails to compile here, see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            looks,
            arrow_size: _,
            arrow_radius: _,
            arrow_closed_angle: _,
            arrow_open_angle: _,
            gap: _,
            indent: _,
            body_padding: _,
            defaults: _,
        } = self;
        looks.for_each_text(f);
    }

    pub(crate) fn arrow_angle(&self, openness: f32) -> f32 {
        let t = openness.clamp(0.0, 1.0);
        self.arrow_closed_angle + (self.arrow_open_angle - self.arrow_closed_angle) * t
    }

    /// A header transparent at rest, picking up a surface fill on hover.
    pub fn from_palette(p: &Palette) -> Self {
        let radius = Corners::all(4.0);
        Self {
            looks: StatefulLook {
                normal: WidgetLook {
                    background: Background::NONE,
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    background: Background::rounded(p.element_mid, radius),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    background: Background::rounded(p.element_strong, radius),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            arrow_size: Vec2::new(9.0, 9.0),
            arrow_radius: 1.5,
            arrow_closed_angle: -FRAC_PI_2,
            arrow_open_angle: 0.0,
            gap: 8.0,
            indent: 17.0,
            body_padding: Spacing::new(0.0, 4.0, 0.0, 4.0),
            defaults: SlotDefaults {
                padding: Spacing::new(4.0, 4.0, 4.0, 4.0),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl Default for ExpanderTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

impl ThemeSlot for ExpanderTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, (): ()) -> &WidgetLook {
        self.looks.pick(response, response.pressed())
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
