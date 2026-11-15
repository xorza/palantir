//! The horizontal value slider and what a frame of it reports.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::VAlign;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::slider::SliderTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::bound_num::{BoundNum, MAX_DECIMALS, StoreRule, TravelRule};
use crate::widgets::press_start;
use std::ops::RangeInclusive;

/// Horizontal value slider over a numeric range, binding the same [`BoundNum`] as
/// [`DragValue`](crate::DragValue). The knob sits between two `Fill` leaves
/// weighted `fraction` and `1 − fraction`, so it tracks the resolved width without
/// knowing it at record time; pointer mapping uses last frame's width. The parts are
/// `id.with("fill")`, `id.with("knob")` and `id.with("rest")`, the unfilled remainder, which
/// wears the track's color, as a progress bar's remainder is its `rest`. Visuals come from
/// [`crate::SliderTheme`] (slot `slider`).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Slider<'a> {
    widget: Widget,
    value: BoundNum<'a>,
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
    pub fn new(value: impl Into<BoundNum<'a>>, range: RangeInclusive<f64>) -> Self {
        // An infinite end would make a click store `+inf`.
        let range = domain::f64::range(range);
        Self {
            widget: Widget::hstack()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true)
                .input_scope(AxisKeys::SCOPE),
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
    /// by the integer target. Default `2`. `n` is coerced to at most 15, the digits an `f64`
    /// holds.
    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = if n < MAX_DECIMALS { n } else { MAX_DECIMALS };
        self
    }

    /// Per-instance override of [`Theme::slider`](crate::Theme::slider).
    pub fn style(mut self, s: impl Into<Option<&'a SliderTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the slider and report what the drag did to the bound value. A
    /// [`ValueResponse`] because `left.drag.is_live()` cannot say whether it moved
    /// (a drag pinned at `min`/`max` keeps reporting).
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse {
        let response = self.widget.response(ui);
        let id = response.id;

        let theme = self.style.unwrap_or(&ui.theme().slider);
        let knob = domain::length_at_least(theme.knob_diameter, 1.0);
        let track_h = domain::length_at_least(theme.track_thickness, 0.0);
        let colors = if response.disabled {
            &theme.disabled
        } else {
            &theme.enabled
        };
        let (fill_color, track_color, knob_color) = (colors.fill, colors.track, colors.knob);

        let range = self.range;
        let rule = StoreRule::new(range.min, range.max, self.decimals).step(range.step);
        // The pointer drives the value on every gesture frame, release included.
        let before = self.value.read();
        let pressed = response
            .press_fraction(knob)
            .is_some_and(|at| self.value.commit_value(range.value_at(at.x), rule));
        let keyed = !response.disabled && ui.is_focus_within(id) && {
            let travel = AxisKeys::SLIDER.read(&mut self.widget, ui);
            let keys = TravelRule {
                step: range.key_unit(self.value.unit(self.decimals)),
                page: (range.max - range.min) / 10.0,
                min: range.min,
                max: range.max,
            };
            self.value.commit_travel(travel, keys, rule)
        };
        // Edge, not level: a press and release on the track writes a value without latching a
        // drag, so the release, not a drag's stop, is the pointer's commit.
        let release_moved =
            press_start::moved_since_press(ui, &response, before, self.value.read());
        let fraction = range.fraction_of(self.value.read().widen());

        let pill = Corners::all(track_h * 0.5);
        let fill_chrome = Chrome::rounded(fill_color, pill);
        let track_chrome = Chrome::rounded(track_color, pill);
        let knob_chrome = Chrome::rounded(knob_color, Corners::all(knob * 0.5));

        self.widget
            .configure()
            .default_size((Sizing::FILL, Sizing::fixed(knob)))
            .default_child_align(VAlign::Center);

        let [filled, remainder] = Sizing::split(fraction);
        self.widget.record(ui, None, |ui| {
            let track = Sizing::fixed(track_h);
            Widget::leaf()
                .id(id.with("fill"))
                .size((filled, track))
                .record(ui, Some(&fill_chrome), |_| {});
            let knob = Sizing::fixed(knob);
            Widget::leaf()
                .id(id.with("knob"))
                .size((knob, knob))
                .record(ui, Some(&knob_chrome), |_| {});
            Widget::leaf()
                .id(id.with("rest"))
                .size((remainder, track))
                .record(ui, Some(&track_chrome), |_| {});
        });
        ValueResponse::gesture(response, pressed, keyed, release_moved)
    }
}

impl Configure for Slider<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// The values a slider's track spans, `min` at the left even when it is above `max`,
/// and the step of the grid its values snap to.
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

    /// The value an arrow steps by: the snap step, else a hundredth of the range, but at least
    /// `grain`, one unit of the bound number's last digit. So an integer slider over a range under
    /// 100 still moves, and a half step on an integer cannot round back to where it started.
    /// Signed to the range's direction, so a reversed range steps the same way along the track.
    fn key_unit(self, grain: f64) -> f64 {
        let span = self.max - self.min;
        self.step
            .unwrap_or_else(|| span.abs() / 100.0)
            .max(grain)
            .copysign(span)
    }
}

#[cfg(test)]
mod tests;
