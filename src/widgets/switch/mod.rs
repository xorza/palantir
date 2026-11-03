//! The pill-and-knob boolean toggle, with the checkbox's contract.

use crate::animation::animation_request::AnimationRequest;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::text::text_input::TextInput;
use crate::theme::Theme;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::toggle_row::{Settled, ToggleKind, ToggleRow};
use glam::Vec2;

/// Boolean toggle drawn as a pill track with a sliding knob; takes a `&mut bool` and a click flips it. Visuals come from `theme.switch` ([`crate::ToggleTheme`]). Layout and parts mirror [`crate::Checkbox`] (`HStack [track, label]`, one `Sense::CLICK` target), and the knob is `id.with("knob")`; the track is a `Canvas`, and one [`Ui::animate`] row from off to on places the knob and blends its ink.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Switch<'a> {
    row: ToggleRow<'a>,
    value: &'a mut bool,
}

impl<'a> Switch<'a> {
    /// A switch bound to `value`, which a click flips.
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        Self {
            row: ToggleRow::new(),
            value,
        }
    }

    /// The text drawn right of the track; empty (the default) records no text child.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.row.label = label.into();
        self
    }

    /// Per-instance override of [`Theme::switch`](crate::Theme::switch).
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.row.style = s.into();
        self
    }

    /// Record the row and report whether this frame flipped the bound `bool` (`committed == changed`).
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        self.row.show(ui, Knob(self.value))
    }
}

impl Configure for Switch<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.row.widget.configure()
    }
}

/// A pill track with a knob that slides to the end while set.
#[derive(Debug)]
struct Knob<'a>(&'a mut bool);

impl ToggleKind for Knob<'_> {
    fn settle(self, activated: bool) -> Settled {
        Settled::flip(self.0, activated)
    }

    fn slot(theme: &Theme) -> &ToggleTheme {
        &theme.switch
    }

    /// A `Canvas`, so the knob can be absolutely positioned; the width does not depend on the
    /// border, so it resolves before the body.
    fn boxed(slot: &ToggleTheme) -> Widget {
        let track = SwitchTrack::from_slot(slot);
        Widget::canvas().size((Sizing::fixed(track.width()), Sizing::fixed(track.height)))
    }

    fn pill(slot: &ToggleTheme) -> Option<f32> {
        Some(SwitchTrack::from_slot(slot).height * 0.5)
    }

    fn paint(ui: &mut Ui, id: WidgetId, slot: &ToggleTheme, chrome: &Chrome, checked: bool) {
        // The track's content box starts where the layout's fold of its border puts it.
        let [border, ..] = chrome.fold_padding(Spacing::ZERO).to_array();
        let geometry = SwitchTrack::from_slot(slot).knob(border);
        let knob_id = id.with("knob");
        // One row from off to on, as Flutter's `Switch` runs one position: the ends are read each
        // frame, so a theme that moves the track or changes an ink carries the knob with it, and
        // the row settles on a share of the travel, not on pixels.
        let on = ui
            .animate(
                AnimationRequest::new(knob_id, "on", f32::from(checked)),
                slot.defaults.transition,
            )
            .value;
        let knob_x = Lerp::lerp(geometry.off_x, geometry.on_x, on);
        // A spring slides the knob past an end, but a color past an end leaves the two inks.
        let knob_color = Lerp::lerp(slot.unchecked_indicator, slot.indicator, on.clamp(0.0, 1.0));
        let knob_chrome = Chrome::rounded(knob_color, Corners::all(geometry.knob * 0.5));
        Widget::leaf()
            .id(knob_id)
            .size((Sizing::fixed(geometry.knob), Sizing::fixed(geometry.knob)))
            .at(Vec2::new(knob_x, geometry.knob_y))
            .record(ui, Some(&knob_chrome), |_| {});
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
struct KnobGeometry {
    knob: f32,
    off_x: f32,
    on_x: f32,
    knob_y: f32,
}

impl SwitchTrack {
    const fn from_slot(slot: &ToggleTheme) -> Self {
        Self {
            height: domain::length_at_least(slot.box_size, 1.0),
            inset: domain::length_at_least(slot.indicator_inset, 0.0),
            aspect: slot.track_aspect,
        }
    }

    /// Needs no border, unlike [`Self::knob`], so it sizes the track before the body runs.
    const fn width(self) -> f32 {
        self.height * self.aspect
    }

    /// The knob is `height - 2 * inset`, floored at 2 px. The track's `border` insets the Canvas content box on
    /// every side (`Tree::open_node`), so it is subtracted back out to keep the knob `inset` from the pill's edge.
    fn knob(self, border: f32) -> KnobGeometry {
        let knob = (self.height - 2.0 * self.inset).max(2.0);
        KnobGeometry {
            knob,
            off_x: self.inset - border,
            on_x: self.width() - knob - self.inset - border,
            knob_y: self.inset - border,
        }
    }
}

#[cfg(test)]
mod tests;
