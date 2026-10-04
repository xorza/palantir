//! The horizontal value slider, and what a frame of it reports about the
//! value it writes through.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::drag_num::DragNum;
use crate::widgets::theme::slider::SliderTheme;
use std::ops::RangeInclusive;

/// Horizontal value slider over a numeric range. Takes the same
/// [`DragNum`] binding [`DragValue`](crate::DragValue) does — `&mut i64`
/// or `&mut f64` — so one number can be scrubbed or slid without
/// changing its type. Dragging (or clicking) the track moves the value.
/// The knob position is derived from it with the same two-`Fill`-leaf
/// trick [`crate::ProgressBar`] uses — `Fill(fraction)` left of the knob,
/// `Fill(1 − fraction)` right — so it tracks the resolved width without
/// the widget knowing it at record time. Pointer→value mapping uses last
/// frame's arranged width (one-frame lag, invisible at interactive
/// rates). Visuals come from [`crate::SliderTheme`] (theme slot
/// `slider`).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Slider<'a> {
    widget: Widget,
    value: DragNum<'a>,
    min: f64,
    max: f64,
    step: Option<f64>,
    decimals: usize,
    style: Option<&'a SliderTheme>,
}

impl<'a> Slider<'a> {
    /// A constructor argument takes what the widget cannot work without,
    /// and a builder step takes the rest. A slider maps a track position
    /// onto its bounds, so it has no meaning without them — where an
    /// unbounded scrub is the drag value's default, and so
    /// [`DragValue::range`](crate::DragValue::range) is a builder step.
    ///
    /// # Panics
    ///
    /// Panics unless both ends of `range` are finite.
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>, range: RangeInclusive<f64>) -> Self {
        // A track maps its fraction onto the range, and an infinite end
        // maps every fraction past zero to infinity or NaN: a click would
        // store `+inf` in the bound value.
        assert!(domain::is_range(&range), "{}", domain::RANGE_RULE);
        Self {
            widget: Widget::hstack()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            value: value.into(),
            min: *range.start(),
            max: *range.end(),
            step: None,
            decimals: 2,
            style: None,
        }
    }

    /// Snap the value to multiples of `step`, anchored at `min`.
    /// Continuous by default.
    ///
    /// # Panics
    ///
    /// Panics unless `step` is finite and greater than zero. A slider
    /// that should not snap simply never calls this — there is no second
    /// spelling of "off".
    #[track_caller]
    pub const fn step(mut self, step: f64) -> Self {
        assert!(step.is_finite() && step > 0.0, "{}", domain::POSITIVE_RULE);
        self.step = Some(step);
        self
    }

    /// Digits after the decimal point a float target's committed value
    /// keeps, so a drag never stores a long tail. Ignored by the integer
    /// target, which rounds whole. Default `2`, matching
    /// [`DragValue::decimals`](crate::DragValue::decimals).
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `slider`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a SliderTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the slider and report what the drag did to the bound
    /// value.
    ///
    /// A [`ValueResponse`] rather than a bare [`Response`] because the
    /// widget writes through the caller's number: they have no other way to
    /// tell whether the value moved this frame. `left.drag.dragging()`
    /// does not answer it — a drag pinned at `min`/`max` keeps reporting
    /// while the value stays put. The same type
    /// [`DragValue`](crate::DragValue) returns, so the two value-editing
    /// widgets read alike.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        let theme = self.style.unwrap_or(&ui.theme().slider);
        let knob = domain::length_at_least(theme.knob_size, 1.0);
        let track_h = domain::length_at_least(theme.track_thickness, 0.0);
        let fill_color = theme.fill;
        let track_color = theme.track;
        let knob_color = theme.knob;

        // Pointer drives the value on every frame of the gesture, the
        // release included, and the knob is the band its centre travels
        // inside. Replaying the value on the release needs no retained
        // anchor the way `DragValue` does: it is a function of the
        // pointer, not of accumulated travel.
        let mut changed = false;
        if let Some(at) = response.press_fraction(knob) {
            let v = snap_to_step(
                fraction_to_value(at.x, self.min, self.max),
                self.min,
                self.step,
            );
            changed = self
                .value
                .commit_value(v, self.decimals, self.min, self.max);
        }
        // A key press is a whole edit of its own, so it commits at once.
        let keyed = !response.disabled
            && ui.focus_within(id)
            && key_target(ui, self.value.read().widen(), self.min, self.max, self.step)
                .is_some_and(|to| {
                    self.value.commit_value(
                        snap_to_step(to, self.min, self.step),
                        self.decimals,
                        self.min,
                        self.max,
                    )
                });
        changed |= keyed;
        // Edge, not level: the frame the gesture ends is the one a caller
        // treats as a single undoable edit. Every release ends one, so
        // this reads `left.released()` and not `drag.stopped()` — a press
        // and release on the track writes a value and never latches a
        // drag, and that edit owes a commit like any other.
        let committed = !response.disabled && (response.left.released() || keyed);
        let fraction = value_to_fraction(self.value.read().widen(), self.min, self.max);

        let pill = Corners::all(track_h * 0.5);
        let fill_bg = Background::rounded(fill_color, pill);
        let track_bg = Background::rounded(track_color, pill);
        let knob_bg = Background::rounded(knob_color, Corners::all(knob * 0.5));

        self.widget
            .configure()
            .default_size((Sizing::FILL, Sizing::fixed(knob)))
            .child_align(Align::v(VAlign::Center));

        // The knob sits between two track segments whose weights
        // partition the span, so its position follows the resolved width
        // without this widget knowing that width at record time.
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
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl Configure for Slider<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

/// Fraction (0..1) of the way from `min` to `max` that `value` sits.
///
/// The share is dimensionless, but the two differences it divides are
/// not: they are in the caller's units, and an app binds units as far
/// either side of a pixel as it likes. So the division and the clamp
/// stay in `f64`, the domain the value and its range live in, and only
/// the settled share narrows. Narrowed first, a whole class of ranges
/// lands on one end of the track — one under the tolerance a share of a
/// *distance* answers to, one past `f32`'s reach, one whose share alone
/// is past it.
///
/// The end of it is [`domain::fraction_or`](crate::widget::domain::fraction_or)'s policy in the value
/// domain: a share that is no share reads as the low end, and every
/// other share is pinned into `0..=1`. Non-finite covers the ranges
/// geometry can collapse — a range whose ends coincide divides by zero,
/// and an unbounded one divides infinity by itself. Both of this
/// widget's fractions are then total and answer a range or a pointer
/// that names no share the same way, since
/// [`ResponseState::press_fraction`](crate::ResponseState::press_fraction)
/// ends in that same fallback.
///
/// A reversed range is not degenerate: it runs from `min` at the left of
/// the track to `max` at the right like any other, so its values descend,
/// and this stays the exact inverse of [`fraction_to_value`] there too.
fn value_to_fraction(value: f64, min: f64, max: f64) -> f32 {
    let share = (value - min) / (max - min);
    if share.is_finite() {
        share.clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Inverse of [`value_to_fraction`]: the value at `fraction` of the
/// range. Taken in `f64` so a wide range keeps the precision the bound
/// value is stored at.
fn fraction_to_value(fraction: f32, min: f64, max: f64) -> f64 {
    min + f64::from(fraction.clamp(0.0, 1.0)) * (max - min)
}

/// Where this frame's keys send a slider now at `at`, or `None` when no key
/// fired. An arrow steps toward `max` (right, up) or `min` (left, down) by
/// `step`, or by a hundredth of the range without one, and Shift makes it
/// ten steps; Page Up and Page Down move a tenth of the range; Home and End
/// jump to `min` and `max`. A reversed range steps the same way along the
/// track, since the direction comes from `max - min`.
///
/// Every chord is sampled rather than short-circuited: `key_pressed` also
/// keeps the chord subscribed for the wake gate.
fn key_target(ui: &mut Ui, at: f64, min: f64, max: f64, step: Option<f64>) -> Option<f64> {
    let span = max - min;
    let unit = step.map_or(span / 100.0, |s| s.copysign(span));
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
        to = if end { max } else { min };
        moved = true;
    }
    moved.then_some(to)
}

/// Snap to the nearest multiple of `step` measured from `min`. A slider
/// with no step passes the value through.
fn snap_to_step(value: f64, min: f64, step: Option<f64>) -> f64 {
    match step {
        Some(s) => min + ((value - min) / s).round() * s,
        None => value,
    }
}

#[cfg(test)]
mod tests;
