//! The pill-and-knob boolean toggle, with the checkbox's contract.

use crate::animation::animation_request::AnimationRequest;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::primitives::text::text_input::TextInput;
use crate::theme::check_state::CheckState;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::toggle_chrome;
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
            widget: toggle_chrome::row(),
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
        let on = toggle_chrome::toggled(
            toggle_chrome::activated(ui, &mut self.widget, &response),
            self.value,
        );
        let changed = on != before;

        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.switch);
        let track = SwitchTrack {
            height: domain::length_at_least(slot.box_size, 1.0),
            inset: domain::length_at_least(slot.indicator_inset, 0.0),
            aspect: slot.track_aspect,
        };
        let knob_color = slot.indicator;
        let anim = slot.defaults.animation;
        let knob_id = id.with("knob");
        let chrome = ToggleChrome {
            row: self.widget,
            response,
            label: self.label,
            plan: slot.plan(&response, CheckState::from(on), theme.text),
            gap: slot.gap,
            // A `Canvas` so the knob can be absolutely positioned; width is border-independent, so it resolves before the body.
            boxed: Widget::canvas()
                .size((Sizing::fixed(track.width()), Sizing::fixed(track.height))),
            pill: Some(track.height * 0.5),
        };
        let response = chrome.record_row(ui, |ui, boxed| {
            // Read off the resolved chrome, since the border animates between looks.
            let border = boxed.border.width;
            let border_inset = if domain::is_invisible(border) {
                0.0
            } else {
                border
            };
            let geom = track.knob(border_inset);

            let target_x = if on { geom.on_x } else { geom.off_x };
            let knob_x = ui.animate(AnimationRequest::new(knob_id, "x", target_x), anim);
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
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// The pill the knob slides in, as the theme sizes it.
#[derive(Clone, Copy, Debug)]
struct SwitchTrack {
    height: f32,
    /// The gap between the knob and the pill's edge.
    inset: f32,
    /// Width over height.
    aspect: f32,
}

/// Knob placement inside the track, relative to its content box.
#[derive(Debug)]
struct KnobGeom {
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
}

impl SwitchTrack {
    /// Needs no border, unlike [`Self::knob`], so it sizes the track before the body runs.
    const fn width(self) -> f32 {
        self.height * self.aspect
    }

    /// The knob is `height - 2 * inset`, floored at 2 px. The track's `border` insets the Canvas content box on
    /// every side (`Tree::open_node`), so it is subtracted back out to keep the knob `inset` from the pill's edge.
    fn knob(self, border: f32) -> KnobGeom {
        let knob = (self.height - 2.0 * self.inset).max(2.0);
        KnobGeom {
            knob,
            off_x: self.inset - border,
            on_x: self.width() - knob - self.inset - border,
            knob_y: self.inset - border,
        }
    }
}

#[cfg(test)]
mod tests;
