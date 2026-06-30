use crate::animation::AnimSpec;
use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::{StatefulLook, WidgetLook};

/// Visuals for two-state toggles ([`crate::Checkbox`],
/// [`crate::RadioButton`], future toggle/segmented controls). Holds a
/// full 4-state look pack per checked branch plus the geometry knobs
/// the widget would otherwise hardcode.
///
/// The chrome painted on the small box/pip comes from
/// `checked.pick(state)` or `unchecked.pick(state)`; the indicator
/// (check polyline, radio dot) uses [`Self::indicator`]. The label
/// reads through the same `pick`'s `text` slot (defaults: `None` on
/// active states inherits `Theme::text`, `disabled` carries
/// `TEXT_DISABLED`) — same flow as Button.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToggleTheme {
    pub unchecked: StatefulLook,
    pub checked: StatefulLook,
    /// Color of the check polyline (Checkbox) or filled dot
    /// (RadioButton). Painted on top of the `checked` chrome.
    pub indicator: Color,
    /// Outer box/pip square side in logical px.
    pub box_size: f32,
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
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.unchecked.for_each_text(f);
        self.checked.for_each_text(f);
    }

    /// Pick the chrome+label look for this `(state, checked)` pair.
    pub fn pick(&self, state: ResponseState, checked: bool) -> &WidgetLook {
        if checked {
            self.checked.pick(state)
        } else {
            self.unchecked.pick(state)
        }
    }

    /// Defaults sized for [`crate::Checkbox`] — 16 px box with a 3 px
    /// corner radius and a `TERMINAL_BG` check.
    pub fn checkbox() -> Self {
        Self::built(3.0, 16.0, 4.0, palette::TERMINAL_BG)
    }

    /// Defaults sized for [`crate::RadioButton`] — 16 px pip with pill
    /// radius (`box_size * 0.5`) and a `TERMINAL_BG` dot.
    pub fn radio() -> Self {
        Self::built(8.0, 16.0, 4.0, palette::TERMINAL_BG)
    }

    /// Defaults sized for [`crate::ToggleSwitch`] — a 20 px-tall pill
    /// track with a white sliding knob. `box_size` is the track height;
    /// the knob diameter is `box_size - 2 * indicator_inset`. Unlike the
    /// checkbox/radio, the switch defaults to an animated knob slide +
    /// track cross-fade — the motion is the point of the control.
    pub fn switch() -> Self {
        let mut t = Self::built(10.0, 20.0, 3.0, palette::TEXT);
        t.anim = Some(AnimSpec::SPRING);
        t
    }

    fn built(corner: f32, box_size: f32, indicator_inset: f32, indicator: Color) -> Self {
        let radius = Corners::all(corner);
        let edge = palette::TEXT_MUTED.with_alpha(0.35);
        let bg = |fill: Color, stroke: Stroke| -> Option<Background> {
            Some(Background {
                fill: fill.into(),
                stroke,
                corners: radius,
                shadow: Shadow::NONE,
            })
        };
        let disabled_text = Some(TextStyle::default().with_color(palette::TEXT_DISABLED));
        let unchecked = StatefulLook {
            normal: WidgetLook {
                background: bg(palette::ELEM_HOVER, Stroke::solid(edge, 1.0)),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(palette::ELEM_ACTIVE, Stroke::solid(edge, 1.0)),
                text: None,
            },
            pressed: WidgetLook {
                background: bg(
                    palette::ELEM_ACTIVE,
                    Stroke::solid(palette::BORDER_FOCUSED, 1.0),
                ),
                text: None,
            },
            disabled: WidgetLook {
                background: bg(palette::ELEM, Stroke::solid(edge.with_alpha(0.18), 1.0)),
                text: disabled_text,
            },
        };
        let acc = palette::ACCENT;
        let checked = StatefulLook {
            normal: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: None,
            },
            hovered: WidgetLook {
                background: bg(acc, Stroke::ZERO),
                text: None,
            },
            pressed: WidgetLook {
                background: bg(acc, Stroke::solid(palette::BORDER_FOCUSED, 1.0)),
                text: None,
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
            row_gap: 8.0,
            anim: None,
        }
    }
}
