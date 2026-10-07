//! What the three two-state toggles wear; one theme serves checkbox, radio and switch.

use crate::animation::animation_spec::AnimationSpec;
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
use glam::Vec2;

/// Visuals for [`crate::Checkbox`], [`crate::RadioButton`] and [`crate::Switch`]: a 4-state look pack per checked branch plus geometry knobs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToggleTheme {
    /// Look when unchecked.
    pub unchecked: StatefulLook,
    /// Look when checked.
    pub checked: StatefulLook,
    /// Colour of the check polyline or radio dot, painted over the `checked` chrome.
    pub indicator: RgbaF32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    /// Box side.
    pub box_size: f32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    /// Stroke width of the indicator.
    pub indicator_width: f32,
    /// The check polyline's three points (Checkbox only) as fractions of [`Self::box_size`], so the tick keeps its proportions at any size.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset_points3")]
    pub check_points: [Vec2; 3],
    /// Inset of the filled dot inside the pip (RadioButton); dot side is `box_size - 2 * indicator_inset`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub indicator_inset: f32,
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    /// Gap between box and label.
    pub gap: f32,
    /// Track width as a multiple of its height ([`crate::Switch`] only, ~7:4); `1.0` for the square checkbox and radio.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub track_aspect: f32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ToggleTheme {
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            unchecked,
            checked,
            indicator: _,
            box_size: _,
            indicator_width: _,
            check_points: _,
            indicator_inset: _,
            gap: _,
            track_aspect: _,
            defaults: _,
        } = self;
        unchecked.for_each_text(f);
        checked.for_each_text(f);
    }

    /// [`Self::check_points`] scaled to [`Self::box_size`], in box-local pixels.
    pub(crate) fn check_polyline(&self) -> [Vec2; 3] {
        self.check_points.map(|p| p * self.box_size)
    }

    /// Defaults for [`crate::Checkbox`]: 16 px box, 3 px corner radius.
    pub fn checkbox(p: &Palette) -> Self {
        Self::built(
            ToggleGeometry {
                corner: 3.0,
                box_size: 16.0,
                indicator_inset: 4.0,
            },
            p.window_background,
            p,
        )
    }

    /// Defaults for [`crate::RadioButton`]: 16 px pip with pill radius.
    pub fn radio(p: &Palette) -> Self {
        Self::built(
            ToggleGeometry {
                corner: 8.0,
                box_size: 16.0,
                indicator_inset: 4.0,
            },
            p.window_background,
            p,
        )
    }

    /// Defaults for [`crate::Switch`]: a 20 px-tall pill track with a white knob, animated slide and cross-fade; `box_size` is the track height.
    pub fn switch(p: &Palette) -> Self {
        let mut t = Self::built(
            ToggleGeometry {
                corner: 10.0,
                box_size: 20.0,
                indicator_inset: 3.0,
            },
            p.text,
            p,
        );
        t.track_aspect = 1.75;
        t.defaults.animation = Some(AnimationSpec::SPRING);
        t
    }

    fn built(geometry: ToggleGeometry, indicator: RgbaF32, p: &Palette) -> Self {
        let ToggleGeometry {
            corner,
            box_size,
            indicator_inset,
        } = geometry;
        let radius = Corners::all(corner);
        let edge = p.border_strong();
        let bg =
            |fill: RgbaF32, stroke: Stroke| Background::rounded(fill, radius).with_border(stroke);
        let disabled_text = TextStyleOverrides::NONE.with_color(p.text_disabled);
        let unchecked = StatefulLook {
            normal: WidgetLook {
                background: bg(p.element_mid, Stroke::new(edge, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                background: bg(p.element_strong, Stroke::new(edge, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                background: bg(p.element_strong, Stroke::new(p.border_focused, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                background: bg(p.element, Stroke::new(p.border_soft(), 1.0)),
                text: disabled_text,
            },
        };
        let acc = p.accent;
        let checked = StatefulLook {
            normal: WidgetLook {
                background: bg(acc, Stroke::NONE),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                background: bg(acc, Stroke::NONE),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                background: bg(acc, Stroke::new(p.border_focused, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                background: bg(acc.with_alpha(0.45), Stroke::NONE),
                text: disabled_text,
            },
        };
        Self {
            unchecked,
            checked,
            indicator,
            box_size,
            indicator_width: 2.0,
            indicator_inset,
            check_points: [
                Vec2::new(3.5 / 16.0, 8.5 / 16.0),
                Vec2::new(7.0 / 16.0, 12.0 / 16.0),
                Vec2::new(12.5 / 16.0, 4.5 / 16.0),
            ],
            gap: 8.0,
            track_aspect: 1.0,
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

/// The three same-typed lengths [`ToggleTheme::built`] would otherwise take positionally, where any two swap and still compile.
#[derive(Clone, Copy, Debug)]
struct ToggleGeometry {
    /// Corner radius of the box/pip chrome; `box_size / 2` makes the pill.
    corner: f32,
    box_size: f32,
    indicator_inset: f32,
}

impl ThemeSlot for ToggleTheme {
    type Pick = bool;

    /// The checked or unchecked pack, then its state (`active` = pressed).
    fn look(&self, response: &ResponseState, checked: bool) -> &WidgetLook {
        let pack = if checked {
            &self.checked
        } else {
            &self.unchecked
        };
        pack.pick(response, response.pressed())
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
