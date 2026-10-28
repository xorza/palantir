//! What a disclosure header wears, and how far its body is inset.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;

/// Visuals for [`crate::Expander`]; the header takes button-style four-state looks.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExpanderTheme {
    /// The header's four per-state looks; `flatten` keeps theme files flat (`[expander.normal]`).
    #[serde(flatten)]
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
    /// Gap between arrow and label.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub gap: f32,
    /// Indent of the body.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub indent: f32,
    /// Padding of the body.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
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
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let radius = Corners::all(m.radius.control);
        let arrow_size = Vec2::new(9.0, 9.0);
        let gap = m.space.sm;
        Self {
            looks: StatefulLook {
                normal: WidgetLook {
                    chrome: Chrome::NONE,
                    text: TextStyleOverrides::NONE,
                },
                hovered: WidgetLook {
                    chrome: Chrome::rounded(p.element_mid, radius),
                    text: TextStyleOverrides::NONE,
                },
                active: WidgetLook {
                    chrome: Chrome::rounded(p.element_strong, radius),
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: Chrome::NONE,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            arrow_size,
            arrow_radius: 1.5,
            arrow_closed_angle: -FRAC_PI_2,
            arrow_open_angle: 0.0,
            gap,
            indent: arrow_size.x + gap,
            body_padding: Spacing::new(0.0, m.space.xs, 0.0, m.space.xs),
            defaults: SlotDefaults {
                padding: Spacing::all(m.space.xs),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl Default for ExpanderTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DEFAULT, &Metrics::DEFAULT)
    }
}

impl ThemeSlot for ExpanderTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, (): ()) -> &WidgetLook {
        self.looks.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
