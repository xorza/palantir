//! What the three two-state toggles wear; one theme serves checkbox, radio and switch.

use crate::animation::animation_spec::AnimationSpec;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::check_state::CheckState;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::theme::toggle_kind::ToggleKind;
use glam::Vec2;

/// Visuals for [`crate::Checkbox`], the pips of [`crate::RadioGroup`] and [`crate::Switch`]: a 4-state look pack per checked branch plus geometry knobs.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
pub struct ToggleTheme {
    /// Look when unchecked.
    pub unchecked: StatefulLook,
    /// Look when checked.
    pub checked: StatefulLook,
    /// Color of the check polyline, the radio dot, or the switch knob over the `checked` chrome.
    pub indicator: RgbaF32,
    /// Color of the switch knob over the `unchecked` chrome ([`crate::Switch`] only; a checkbox
    /// and a radio paint no mark when off).
    pub unchecked_indicator: RgbaF32,
    /// Box side.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub box_size: f32,
    /// Stroke width of the indicator.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub indicator_width: f32,
    /// The check polyline's three points (Checkbox only) as fractions of [`Self::box_size`], so the tick keeps its proportions at any size.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset_points3")]
    pub check_points: [Vec2; 3],
    /// Inset of the filled dot inside a radio pip; dot side is `box_size - 2 * indicator_inset`.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub indicator_inset: f32,
    /// Gap between box and label.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// Track width as a multiple of its height ([`crate::Switch`] only, ~7:4); `1.0` for the square checkbox and radio.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub track_aspect: f32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ToggleTheme {
    /// [`Self::box_size`] as the box paints it: a pixel at least, so a box too small to see still
    /// takes a hit and paints its mark inside itself.
    pub(crate) const fn box_side(&self) -> f32 {
        domain::length_at_least(self.box_size, 1.0)
    }

    /// [`Self::check_points`] scaled to the [box's side](Self::box_side), in box-local pixels.
    pub(crate) fn check_polyline(&self) -> [Vec2; 3] {
        let side = self.box_side();
        self.check_points.map(|p| p * side)
    }

    /// The recipe for `kind` over `p` and `m`; [`ToggleKind`] names each one's geometry.
    pub fn from_parts(kind: ToggleKind, p: &Palette, m: &Metrics) -> Self {
        match kind {
            ToggleKind::Checkbox => ToggleGeometry {
                corner: m.radius.control,
                box_size: 16.0,
                indicator_inset: 4.0,
            }
            .theme(p, m),
            ToggleKind::Radio => ToggleGeometry {
                corner: 8.0,
                box_size: 16.0,
                indicator_inset: 4.0,
            }
            .theme(p, m),
            ToggleKind::Switch => {
                let mut t = ToggleGeometry {
                    corner: 10.0,
                    box_size: 20.0,
                    indicator_inset: 3.0,
                }
                .theme(p, m);
                t.track_aspect = 1.75;
                t.defaults.transition = AnimationSpec::SPRING;
                t
            }
        }
    }

    /// The built-in look of `kind` in `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub fn stock(kind: ToggleKind, scheme: ColorScheme) -> Self {
        Self::from_parts(kind, Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }
}

/// A toggle's three same-typed lengths, named, so no two swap and still compile; [`Self::theme`] builds the toggle's theme from them.
#[derive(Clone, Copy, Debug)]
struct ToggleGeometry {
    /// Corner radius of the box/pip chrome; `box_size / 2` makes the pill.
    corner: f32,
    box_size: f32,
    indicator_inset: f32,
}

impl ToggleGeometry {
    /// The toggle theme of this geometry: its mark in `on_accent` over the accent, and a switch's
    /// knob in `text` over the unchecked track.
    fn theme(self, p: &Palette, m: &Metrics) -> ToggleTheme {
        let Self {
            corner,
            box_size,
            indicator_inset,
        } = self;
        let radius = Corners::all(corner);
        let edge = p.border_strong();
        let hairline = m.stroke.hairline;
        let chrome =
            |fill: RgbaF32, stroke: Stroke| Chrome::rounded(fill, radius).with_border(stroke);
        let disabled_text = TextStyleOverrides::NONE.with_color(p.text_disabled);
        let unchecked = StatefulLook {
            normal: WidgetLook {
                chrome: chrome(p.element_mid, Stroke::new(edge, hairline)),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                chrome: chrome(p.element_strong, Stroke::new(edge, hairline)),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                chrome: chrome(p.element_strong, Stroke::new(p.border_focused, hairline)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                chrome: chrome(p.element, Stroke::new(p.border_soft(), hairline)),
                text: disabled_text,
            },
        };
        let acc = p.accent;
        let checked = StatefulLook {
            normal: WidgetLook {
                chrome: chrome(acc, Stroke::NONE),
                text: TextStyleOverrides::NONE,
            },
            hovered: WidgetLook {
                chrome: chrome(acc, Stroke::NONE),
                text: TextStyleOverrides::NONE,
            },
            active: WidgetLook {
                chrome: chrome(acc, Stroke::new(p.border_focused, hairline)),
                text: TextStyleOverrides::NONE,
            },
            disabled: WidgetLook {
                chrome: chrome(acc.with_alpha8(115), Stroke::NONE),
                text: disabled_text,
            },
        };
        ToggleTheme {
            unchecked,
            checked,
            indicator: p.on_accent,
            unchecked_indicator: p.text,
            box_size,
            indicator_width: m.stroke.line,
            indicator_inset,
            check_points: [
                Vec2::new(3.5 / 16.0, 8.5 / 16.0),
                Vec2::new(7.0 / 16.0, 12.0 / 16.0),
                Vec2::new(12.5 / 16.0, 4.5 / 16.0),
            ],
            gap: m.space.sm,
            track_aspect: 1.0,
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                transition: AnimationSpec::SNAP,
            },
        }
    }
}

impl ThemeSlot for ToggleTheme {
    type Pick = CheckState;

    /// The checked or unchecked pack, then its state (`active` = pressed).
    fn look(&self, response: &ResponseState, check: CheckState) -> &WidgetLook {
        let pack = match check {
            CheckState::Checked => &self.checked,
            CheckState::Unchecked => &self.unchecked,
        };
        pack.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
