//! The horizontal value slider and what a frame of it reports.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::drag_num::{DragNum, StoreRule};
use crate::widgets::theme::slider::SliderTheme;
use std::ops::RangeInclusive;

/// Horizontal value slider over a numeric range, binding the same [`DragNum`] as
/// [`DragValue`](crate::DragValue). The knob sits between two `Fill` leaves
/// weighted `fraction` and `1 − fraction`, so it tracks the resolved width without
/// knowing it at record time; pointer mapping uses last frame's width. Visuals come
/// from [`crate::SliderTheme`] (slot `slider`).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Slider<'a> {
    widget: Widget,
    value: DragNum<'a>,
    range: SliderRange,
    decimals: usize,
    style: Option<&'a SliderTheme>,
}

impl<'a> Slider<'a> {
    /// `range` is required (a slider maps a track position onto its bounds);
    /// [`DragValue::range`](crate::DragValue::range) is a builder step.
    ///
    /// # Panics
    ///
    /// Panics unless both ends of `range` are finite.
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>, range: RangeInclusive<f64>) -> Self {
        // An infinite end would make a click store `+inf`.
        assert!(domain::f64::is_range(&range), "{}", domain::RANGE_RULE);
        Self {
            widget: Widget::hstack()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            value: value.into(),
            range: SliderRange {
                min: *range.start(),
                max: *range.end(),
                step: None,
            },
            decimals: 2,
            style: None,
        }
    }

    /// Snap the value to multiples of `step`, anchored at `min`. Continuous by
    /// default.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is finite and greater than zero.
    #[track_caller]
    pub const fn step(mut self, step: f64) -> Self {
        self.range.step = Some(domain::f64::positive(step));
        self
    }

    /// Digits after the decimal point a float target keeps when committed; ignored
    /// by the integer target. Default `2`.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `slider`.
    pub fn style(mut self, s: impl Into<Option<&'a SliderTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the slider and report what the drag did to the bound value. A
    /// [`ValueResponse`] because `left.drag.is_live()` cannot say whether it moved
    /// (a drag pinned at `min`/`max` keeps reporting).
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        let theme = self.style.unwrap_or(&ui.theme().slider);
        let knob = domain::length_at_least(theme.knob_size, 1.0);
        let track_h = domain::length_at_least(theme.track_thickness, 0.0);
        let fill_color = theme.fill;
        let track_color = theme.track;
        let knob_color = theme.knob;

        let range = self.range;
        let rule = StoreRule::new(range.min, range.max, self.decimals);
        // The pointer drives the value on every gesture frame, release included; no
        // anchor is retained.
        let pressed = response.press_fraction(knob).is_some_and(|at| {
            self.value
                .commit_value(range.snapped(range.value_at(at.x)), rule)
        });
        let keyed = !response.disabled
            && ui.is_focus_within(id)
            && range
                .key_target(ui, self.value.read().widen())
                .is_some_and(|to| self.value.commit_value(range.snapped(to), rule));
        let changed = pressed | keyed;
        // Edge, not level: a press and release on the track writes a value without
        // latching a drag, so read `left.released()`, not `drag.stopped()`.
        let committed = !response.disabled && (response.left.released() || keyed);
        let fraction = range.fraction_of(self.value.read().widen());

        let pill = Corners::all(track_h * 0.5);
        let fill_bg = Background::rounded(fill_color, pill);
        let track_bg = Background::rounded(track_color, pill);
        let knob_bg = Background::rounded(knob_color, Corners::all(knob * 0.5));

        self.widget
            .configure()
            .default_size((Sizing::FILL, Sizing::fixed(knob)))
            .child_align(Align::v(VAlign::Center));

        let [filled, remainder] = Sizing::split(fraction);
        self.widget.record(ui, None, |ui| {
            let track = Sizing::fixed(track_h);
            Widget::leaf()
                .id(id.with("fill"))
                .size((filled, track))
                .record(ui, Some(&fill_bg), |_| {});
            let knob = Sizing::fixed(knob);
            Widget::leaf()
                .id(id.with("knob"))
                .size((knob, knob))
                .record(ui, Some(&knob_bg), |_| {});
            Widget::leaf()
                .id(id.with("track"))
                .size((remainder, track))
                .record(ui, Some(&track_bg), |_| {});
        });
        ValueResponse {
            response: Response::new(id, ui, response),
            changed,
            committed,
        }
    }
}

impl Configure for Slider<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// The values a slider's track spans, `min` at the left even when it is above `max`,
/// and the grid its values snap to.
#[derive(Clone, Copy, Debug)]
struct SliderRange {
    min: f64,
    max: f64,
    step: Option<f64>,
}

impl SliderRange {
    /// Fraction (0..1) of the way from `min` to `max` that `value` sits. Division and
    /// clamp stay in `f64`, as narrowing first would pin whole classes of ranges to one
    /// end. A non-finite share (coincident ends, unbounded range) reads as the low end,
    /// per [`domain::fraction_or`]. A reversed range runs `min` at the left, so values
    /// descend; this is the exact inverse of [`Self::value_at`].
    fn fraction_of(self, value: f64) -> f32 {
        let share = (value - self.min) / (self.max - self.min);
        if share.is_finite() {
            share.clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }

    /// Inverse of [`Self::fraction_of`], in `f64` to keep the bound value's precision.
    fn value_at(self, fraction: f32) -> f64 {
        self.min + f64::from(fraction.clamp(0.0, 1.0)) * (self.max - self.min)
    }

    /// Snap to the nearest multiple of `step` from `min`; no step passes the value
    /// through.
    fn snapped(self, value: f64) -> f64 {
        match self.step {
            Some(s) => self.min + ((value - self.min) / s).round() * s,
            None => value,
        }
    }

    /// Where this frame's keys send a slider now at `at`, or `None` when no key fired.
    /// Arrows step toward `max` (right, up) or `min` (left, down) by `step`, or a
    /// hundredth of the range; Shift makes it ten steps; Page Up/Down move a tenth;
    /// Home/End jump to the ends. Direction comes from `max - min`, so a reversed range
    /// steps the same way along the track. Every chord is sampled, not short-circuited,
    /// as `key_pressed` also keeps it subscribed for the wake gate.
    fn key_target(self, ui: &mut Ui, at: f64) -> Option<f64> {
        let span = self.max - self.min;
        let unit = self.step.map_or(span / 100.0, |s| s.copysign(span));
        let mut to = at;
        let mut moved = false;
        for (key, sign) in [
            (Key::ArrowLeft, -1.0),
            (Key::ArrowDown, -1.0),
            (Key::ArrowRight, 1.0),
            (Key::ArrowUp, 1.0),
        ] {
            let coarse = ui.key_pressed(Shortcut::new(ShortcutMods::SHIFT, key));
            let plain = ui.key_pressed(Shortcut::key(key));
            if coarse {
                to += sign * unit * 10.0;
                moved = true;
            } else if plain {
                to += sign * unit;
                moved = true;
            }
        }
        for (key, sign) in [(Key::PageDown, -1.0), (Key::PageUp, 1.0)] {
            if ui.key_pressed(Shortcut::key(key)) {
                to += sign * span / 10.0;
                moved = true;
            }
        }
        let home = ui.key_pressed(Shortcut::key(Key::Home));
        let end = ui.key_pressed(Shortcut::key(Key::End));
        if home || end {
            to = if end { self.max } else { self.min };
            moved = true;
        }
        moved.then_some(to)
    }
}

#[cfg(test)]
mod tests;
