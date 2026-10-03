//! What the three two-state toggles wear. One theme serves all of them,
//! because a checkbox, a radio and a switch differ in what they draw
//! rather than in what they can be told.

use crate::animation::anim_spec::AnimSpec;
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

/// Visuals for two-state toggles — [`crate::Checkbox`],
/// [`crate::RadioButton`] and [`crate::Switch`]. Holds a full 4-state
/// look pack per checked branch plus the geometry knobs the widget
/// would otherwise hardcode.
///
/// The chrome painted on the small box/pip comes from
/// `checked` or `unchecked` by state; the indicator
/// (check polyline, radio dot) uses [`Self::indicator`]. The label
/// reads through the picked look's `text` overrides (defaults: none on
/// active states, so they inherit `Theme::text`; `disabled` names the
/// palette's `text_disabled` colour alone) — same flow as Button.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToggleTheme {
    /// Chrome for the box or pip while the value is `false`.
    pub unchecked: StatefulLook,
    /// Chrome for it while the value is `true`.
    pub checked: StatefulLook,
    /// RgbaF32 of the check polyline (Checkbox) or filled dot
    /// (RadioButton). Painted on top of the `checked` chrome.
    pub indicator: RgbaF32,
    /// Outer box/pip square side in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub box_size: f32,
    /// Stroke width of the check polyline (Checkbox).
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub indicator_stroke: f32,
    /// The check polyline's three points (Checkbox only), as fractions
    /// of [`Self::box_size`] — origin top-left, `1.0` the far edge. Unit
    /// space rather than pixels so the tick keeps its proportions at any
    /// box size, and so the shape carries no reference size of its own
    /// to fall out of step with `box_size`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset_points3")]
    pub check_pts: [Vec2; 3],
    /// Inset of the filled dot inside the pip (RadioButton).
    /// Dot side = `box_size - 2 * indicator_inset`.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub indicator_inset: f32,
    /// Gap between the box/pip and the label.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Track width as a multiple of its height — [`crate::Switch`]
    /// only, where `box_size` is the track height. A switch reads as a
    /// switch (rather than a checkbox) at roughly 7:4. `1.0` on the
    /// checkbox and radio bundles, whose box is square.
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
            indicator_stroke: _,
            check_pts: _,
            indicator_inset: _,
            gap: _,
            track_aspect: _,
            defaults: _,
        } = self;
        unchecked.for_each_text(f);
        checked.for_each_text(f);
    }

    /// [`Self::check_pts`] scaled to [`Self::box_size`] — the polyline
    /// [`crate::Checkbox`] draws, in box-local pixels.
    pub(crate) fn check_polyline(&self) -> [Vec2; 3] {
        self.check_pts.map(|p| p * self.box_size)
    }

    /// Defaults sized for [`crate::Checkbox`] — 16 px box with a 3 px
    /// corner radius and a `window_bg` check.
    pub fn checkbox(p: &Palette) -> Self {
        Self::built(
            ToggleGeometry {
                corner: 3.0,
                box_size: 16.0,
                indicator_inset: 4.0,
            },
            p.window_bg,
            p,
        )
    }

    /// Defaults sized for [`crate::RadioButton`] — 16 px pip with pill
    /// radius (`box_size * 0.5`) and a `window_bg` dot.
    pub fn radio(p: &Palette) -> Self {
        Self::built(
            ToggleGeometry {
                corner: 8.0,
                box_size: 16.0,
                indicator_inset: 4.0,
            },
            p.window_bg,
            p,
        )
    }

    /// Defaults sized for [`crate::Switch`] — a 20 px-tall pill
    /// track with a white sliding knob. `box_size` is the track height;
    /// the knob diameter is `box_size - 2 * indicator_inset`. Unlike the
    /// checkbox/radio, the switch defaults to an animated knob slide +
    /// track cross-fade — the motion is the point of the control.
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
        t.defaults.anim = Some(AnimSpec::SPRING);
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
                background: bg(p.elem_mid, Stroke::new(edge, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                background: bg(p.elem_strong, Stroke::new(edge, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                background: bg(p.elem_strong, Stroke::new(p.border_focused, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                background: bg(p.elem, Stroke::new(p.border_soft(), 1.0)),
                text: disabled_text,
            },
        };
        let acc = p.accent;
        let checked = StatefulLook {
            normal: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                background: bg(acc, Stroke::new(p.border_focused, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                background: bg(acc.with_alpha(0.45), Stroke::ZERO),
                text: disabled_text,
            },
        };
        Self {
            unchecked,
            checked,
            indicator,
            box_size,
            indicator_stroke: 2.0,
            indicator_inset,
            check_pts: [
                Vec2::new(3.5 / 16.0, 8.5 / 16.0),
                Vec2::new(7.0 / 16.0, 12.0 / 16.0),
                Vec2::new(12.5 / 16.0, 4.5 / 16.0),
            ],
            gap: 8.0,
            track_aspect: 1.0,
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }
}

/// The three same-typed lengths [`ToggleTheme::built`] would otherwise
/// take positionally, where any two of them swap and still compile —
/// the reason [`SlotDefaults`] is a struct too.
#[derive(Clone, Copy, Debug)]
struct ToggleGeometry {
    /// Corner radius of the box/pip chrome in logical px. `box_size / 2`
    /// makes the pill the radio and the switch need.
    corner: f32,
    box_size: f32,
    indicator_inset: f32,
}

impl ThemeSlot for ToggleTheme {
    type Pick = bool;

    /// The checked or the unchecked pack, then its state (`active` =
    /// pressed).
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
