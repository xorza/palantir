//! What a disclosure header wears, and how far its body is inset.

use crate::animation::animation_spec::AnimationSpec;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use std::f32::consts::FRAC_PI_2;

/// Visuals for [`crate::Expander`]; the header takes button-style four-state looks.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
pub struct ExpanderTheme {
    /// The header's four per-state looks; `flatten` keeps theme files flat (`[expander.normal]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// Triangle bounding box, corners included; keep it square unless the angles leave it unturned.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::size")]
    pub arrow_size: Size,
    /// Triangle corner radius; `0.0` is sharp. Rounding keeps the edges, and a radius past the
    /// triangle's inradius paints its incircle.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub arrow_radius: f32,
    /// Arrow angle in radians while closed (default: points at the label).
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub arrow_closed_angle: f32,
    /// Arrow angle while open (default: upright). `0.0` and `-PI` give down-closed, up-open.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::angle")]
    pub arrow_open_angle: f32,
    /// Gap between arrow and label.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// Indent of the body.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub indent: f32,
    /// Padding of the body.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub body_padding: Spacing,
    /// `animation` is `None` by default, so a reveal snaps.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ExpanderTheme {
    pub(crate) fn arrow_angle(&self, openness: f32) -> f32 {
        Lerp::lerp(
            &self.arrow_closed_angle,
            &self.arrow_open_angle,
            openness.clamp(0.0, 1.0),
        )
    }

    /// A header transparent at rest, picking up a surface fill on hover.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let radius = Corners::all(m.radius.control);
        // Rounding by 1.5 keeps the edges and trims 1.85 px off each axis of this
        // triangle, so the painted arrow reads 9 px.
        let arrow_size = Size::new(11.0, 11.0);
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
            indent: arrow_size.w + gap,
            body_padding: Spacing::new(0.0, m.space.xs, 0.0, m.space.xs),
            defaults: SlotDefaults {
                padding: Spacing::all(m.space.xs),
                margin: Spacing::ZERO,
                transition: AnimationSpec::SNAP,
            },
        }
    }

    /// The built-in look of `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
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
