//! The horizontal value slider and what a frame of it reports.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::slider::SliderTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::{AxisEnd, AxisKeys, KeyPair, Travel};
use crate::widgets::drag_num::{DragNum, MAX_DECIMALS, StoreRule};
use std::ops::RangeInclusive;

/// Horizontal value slider over a numeric range, binding the same [`DragNum`] as
/// [`DragValue`](crate::DragValue). The knob sits between two `Fill` leaves
/// weighted `fraction` and `1 − fraction`, so it tracks the resolved width without
/// knowing it at record time; pointer mapping uses last frame's width. The parts are
/// `id.with("fill")`, `id.with("knob")` and `id.with("track")`. Visuals come from
/// [`crate::SliderTheme`] (slot `slider`).
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
        let id = response.id;

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
        let keyed = !response.disabled && ui.is_focus_within(id) && {
            let travel = KEYS.read(&mut self.widget, ui);
            !travel.is_still() && self.commit_travel(travel, rule)
        };
        let changed = pressed | keyed;
        // Edge, not level: a press and release on the track writes a value without
        // latching a drag, so read `left.released()`, not `drag.stopped()`.
        let committed = !response.disabled && (response.left.released() || keyed);
        let fraction = range.fraction_of(self.value.read().widen());

        let pill = Corners::all(track_h * 0.5);
        let fill_chrome = Chrome::rounded(fill_color, pill);
        let track_chrome = Chrome::rounded(track_color, pill);
        let knob_chrome = Chrome::rounded(knob_color, Corners::all(knob * 0.5));

        self.widget
            .configure()
            .default_size((Sizing::FILL, Sizing::fixed(knob)))
            .default_child_align(Align::v(VAlign::Center));

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
                .id(id.with("track"))
                .size((remainder, track))
                .record(ui, Some(&track_chrome), |_| {});
        });
        ValueResponse {
            response: Response::new(ui, response),
            changed,
            committed,
        }
    }
}

impl Slider<'_> {
    /// Store where `travel` sends the value. Without a snap step the steps add to the stored
    /// number through [`DragNum::commit_drag`], so an integer past 2^53 moves by whole steps
    /// where a round trip through `f64` could store a lower value.
    fn commit_travel(&mut self, travel: Travel, rule: StoreRule) -> bool {
        let range = self.range;
        match travel.end {
            Some(AxisEnd::Start) => self.value.commit_value(range.snapped(range.min), rule),
            Some(AxisEnd::End) => self.value.commit_value(range.snapped(range.max), rule),
            None => {
                let unit = range.key_unit(self.value.unit(self.decimals));
                let by = f64::from(travel.steps) * unit
                    + f64::from(travel.pages) * (range.max - range.min) / 10.0;
                let at = self.value.read();
                match range.step {
                    Some(_) => self
                        .value
                        .commit_value(range.snapped(at.widen() + by), rule),
                    None => self.value.commit_drag(at, by, rule),
                }
            }
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

    /// Snap to the nearest multiple of `step` from `min` inside the range, so a span that is no
    /// multiple of the step ends on its last grid value (`0..=10` by 4 takes 10 to 8); no step
    /// passes the value through.
    fn snapped(self, value: f64) -> f64 {
        let Some(s) = self.step else {
            return value;
        };
        let span = self.max - self.min;
        let count = span / s;
        // A span that is a whole number of steps divides a few ulps off (`0.3 / 0.1` is
        // `2.9999999999999996`). The span and the product each round by half an ulp of the span,
        // so a product that close to it is the whole count.
        let last = if (count.round() * s - span).abs() <= 2.0 * f64::EPSILON * span.abs() {
            count.round()
        } else {
            count.trunc()
        };
        let k = ((value - self.min) / s).round();
        let k = if last >= 0.0 {
            k.clamp(0.0, last)
        } else {
            k.clamp(last, 0.0)
        };
        self.min + k * s
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

/// Arrows step toward `max` (right, up) or `min` (left, down); Page Up and Down move a tenth of
/// the range; Home and End jump to the ends.
const KEYS: AxisKeys = AxisKeys {
    steps: &[
        KeyPair {
            back: Key::ArrowLeft,
            forward: Key::ArrowRight,
        },
        KeyPair {
            back: Key::ArrowDown,
            forward: Key::ArrowUp,
        },
    ],
    page: Some(KeyPair {
        back: Key::PageDown,
        forward: Key::PageUp,
    }),
    ends: Some(KeyPair {
        back: Key::Home,
        forward: Key::End,
    }),
};

#[cfg(test)]
mod tests;
