//! The pill-and-knob boolean toggle, with the checkbox's contract.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::primitives::text::text_input::TextInput;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle_chrome::ToggleChrome;
use glam::Vec2;

/// Boolean toggle drawn as a pill track with a sliding knob; takes a `&mut bool` and a click flips it. Visuals come from `theme.switch` ([`crate::ToggleTheme`]). Layout mirrors [`crate::Checkbox`] (`HStack [track, label]`, one `Sense::CLICK` target); the track is a `Canvas` and the knob's x animates through [`Ui::animate`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Switch<'a> {
    widget: Widget,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Switch<'a> {
    /// A switch bound to `value`, which a click flips.
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        Self {
            widget: ToggleChrome::row(),
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    /// The text drawn right of the track; empty (the default) records no text child.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Per-instance override of `switch`; takes an `Option` as readily as a reference.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the row and report whether this frame flipped the bound `bool` (`committed == changed`).
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        let before = *self.value;
        let on = ToggleChrome::toggled(
            ToggleChrome::activated(ui, &mut self.widget, &response),
            self.value,
        );
        let changed = on != before;

        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.switch);
        let track_h = domain::length_at_least(slot.box_size, 1.0);
        let inset = domain::length_at_least(slot.indicator_inset, 0.0);
        let aspect = slot.track_aspect;
        let knob_color = slot.indicator;
        let anim = slot.defaults.animation;
        let knob_id = id.with("knob");
        let chrome = ToggleChrome {
            plan: slot.plan(&response, on, theme.text),
            gap: slot.gap,
            // A `Canvas` so the knob can be absolutely positioned; width is border-independent, so it resolves before the body.
            boxed: Widget::canvas().size((
                Sizing::fixed(track_width(track_h, aspect)),
                Sizing::fixed(track_h),
            )),
            pill: Some(track_h * 0.5),
        };
        let response = chrome.record_row(ui, self.widget, response, self.label, |ui, track| {
            // The track's border insets the Canvas content box on every side (`Tree::open_node`), so `switch_geom` subtracts it back out to keep knob margins measured from the pill's edge. Read off the resolved chrome, since the border animates between looks.
            let border = track.border.width;
            let border_inset = if domain::is_invisible(border) {
                0.0
            } else {
                border
            };
            let geom = switch_geom(track_h, inset, border_inset, aspect);

            let target_x = if on { geom.on_x } else { geom.off_x };
            let knob_x = ui.animate(knob_id, "x", target_x, anim);
            let knob_bg = Background::rounded(knob_color, Corners::all(geom.knob * 0.5));
            let knob = Widget::leaf()
                .id(knob_id)
                .size((Sizing::fixed(geom.knob), Sizing::fixed(geom.knob)))
                .position(Vec2::new(knob_x, geom.knob_y));
            knob.record(ui, Some(&knob_bg), |_| {});
        });
        ValueResponse {
            response,
            changed,
            committed: changed,
        }
    }
}

impl Configure for Switch<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// Knob placement inside the track; the track's own extent is [`track_width`] × `track_h`, needed earlier in `Switch::show`.
#[derive(Debug)]
struct SwitchGeom {
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
}

/// Track width for a `track_h`-tall switch; split out because it needs no border, unlike [`switch_geom`].
fn track_width(track_h: f32, aspect: f32) -> f32 {
    track_h * aspect
}

/// Track/knob geometry from the track height, knob inset and `border` width; the knob is `track_h - 2*inset`, floored at 2 px.
///
/// Returned x/y are content-box-relative: `border` is subtracted because the border insets the Canvas content box (`Tree::open_node`).
fn switch_geom(track_h: f32, inset: f32, border: f32, aspect: f32) -> SwitchGeom {
    let track_w = track_width(track_h, aspect);
    let knob = (track_h - 2.0 * inset).max(2.0);
    SwitchGeom {
        knob,
        off_x: inset - border,
        on_x: track_w - knob - inset - border,
        knob_y: inset - border,
    }
}

#[cfg(test)]
mod tests;
