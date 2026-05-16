use crate::animation::AnimSpec;
use crate::input::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::widget_look::{StatefulLook, WidgetLook};

/// Visuals for two-state toggles ([`crate::Checkbox`],
/// [`crate::RadioButton`], future toggle/segmented controls). Holds a
/// full 4-state look pack per checked branch plus the geometry knobs
/// the widget would otherwise hardcode.
///
/// The chrome painted on the small box/pip comes from
/// `checked.pick(state)` or `unchecked.pick(state)`; the indicator
/// (check polyline, radio dot) uses [`Self::indicator`]. Both branches
/// share one set of geometry — the widget chooses how to interpret
/// them (Checkbox uses `box_radius`; RadioButton ignores it and uses
/// `box_size * 0.5` for a perfect pill).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToggleTheme {
    pub unchecked: StatefulLook,
    pub checked: StatefulLook,
    /// Color of the check polyline (Checkbox) or filled dot
    /// (RadioButton). Painted on top of the `checked` chrome.
    pub indicator: Color,
    /// Indicator alpha multiplier applied when disabled. The chrome
    /// already comes from the `disabled` look; this just dims the
    /// check/dot to match.
    pub indicator_disabled_alpha: f32,
    /// Outer box/pip square side in logical px.
    pub box_size: f32,
    /// Box corner radius (Checkbox only — RadioButton paints a pill).
    pub box_radius: f32,
    /// Stroke width of the check polyline (Checkbox).
    pub indicator_stroke: f32,
    /// Inset of the filled dot inside the pip (RadioButton).
    /// Dot side = `box_size - 2 * indicator_inset`.
    pub indicator_inset: f32,
    /// Gap between the box/pip and the label.
    pub row_gap: f32,
    /// Spec applied to fill/stroke transitions between states and
    /// across checked toggles. Default `None` — animation is opt-in
    /// (matches `ButtonTheme`). Round-trips through serde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

impl ToggleTheme {
    /// Pick the chrome look for this `(state, checked)` pair.
    pub fn pick(&self, state: ResponseState, checked: bool) -> &WidgetLook {
        if checked {
            self.checked.pick(state)
        } else {
            self.unchecked.pick(state)
        }
    }

    /// Resolve the indicator color for `state` (dimmed when disabled).
    pub fn indicator_color(&self, state: ResponseState) -> Color {
        if state.disabled {
            self.indicator
                .with_alpha(self.indicator.a * self.indicator_disabled_alpha)
        } else {
            self.indicator
        }
    }

    fn unchecked_default(radius: Corners) -> StatefulLook {
        // Same fill rhythm as ButtonTheme: ELEM_HOVER at rest, ELEM_ACTIVE
        // on hover, a focused-stroke ring on press, ELEM when disabled.
        let m = palette::TEXT_MUTED;
        let edge = m.with_alpha(0.35);
        let bg = |fill: Color, stroke: Stroke| -> Option<Background> {
            Some(Background {
                fill: fill.into(),
                stroke,
                radius,
                shadow: Shadow::NONE,
            })
        };
        let s = |c: Color, w: f32| Stroke::solid(c, w);
        StatefulLook {
            normal: WidgetLook {
                background: bg(palette::ELEM_HOVER, s(edge, 1.0)),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(palette::ELEM_ACTIVE, s(edge, 1.0)),
                text: None,
            },
            pressed: WidgetLook {
                background: bg(palette::ELEM_ACTIVE, s(palette::BORDER_FOCUSED, 1.0)),
                text: None,
            },
            disabled: WidgetLook {
                background: bg(palette::ELEM, s(edge.with_alpha(0.18), 1.0)),
                text: None,
            },
        }
    }

    fn checked_default(radius: Corners) -> StatefulLook {
        // Filled accent at rest; brighten slightly on hover, ring on
        // press, desaturate when disabled.
        let bg = |fill: Color, stroke: Stroke| -> Option<Background> {
            Some(Background {
                fill: fill.into(),
                stroke,
                radius,
                shadow: Shadow::NONE,
            })
        };
        let s = |c: Color, w: f32| Stroke::solid(c, w);
        let acc = palette::ACCENT;
        StatefulLook {
            normal: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: None,
            },
            pressed: WidgetLook {
                background: bg(acc, s(palette::BORDER_FOCUSED, 1.0)),
                text: None,
            },
            disabled: WidgetLook {
                background: bg(acc.with_alpha(0.45), Stroke::ZERO),
                text: None,
            },
        }
    }

    pub(crate) fn checkbox_default() -> Self {
        let radius = Corners::all(3.0);
        Self {
            unchecked: Self::unchecked_default(radius),
            checked: Self::checked_default(radius),
            indicator: palette::TERMINAL_BG,
            indicator_disabled_alpha: 0.6,
            box_size: 16.0,
            box_radius: 3.0,
            indicator_stroke: 2.0,
            indicator_inset: 4.0,
            row_gap: 8.0,
            anim: None,
        }
    }

    pub(crate) fn radio_default() -> Self {
        // Pill radius — half side length so any box_size produces a
        // perfect circle. Use a sentinel `Corners::all(box_size * 0.5)`.
        let radius = Corners::all(8.0);
        Self {
            unchecked: Self::unchecked_default(radius),
            checked: Self::checked_default(radius),
            indicator: palette::TERMINAL_BG,
            indicator_disabled_alpha: 0.6,
            box_size: 16.0,
            box_radius: 8.0,
            indicator_stroke: 2.0,
            indicator_inset: 4.0,
            row_gap: 8.0,
            anim: None,
        }
    }
}
