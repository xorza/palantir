//! The horizontal value slider, and what a frame of it reports about the
//! value it writes through.

use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::num::F32Ext;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::drag_num::DragNum;
use crate::widgets::response::Response;
use crate::widgets::theme::slider::SliderTheme;
use crate::widgets::value_response::ValueResponse;
use std::ops::RangeInclusive;

/// Horizontal value slider over a numeric range. Takes the same
/// [`DragNum`] binding [`DragValue`](crate::DragValue) does — `&mut i64`
/// or `&mut f64` — so one number can be scrubbed or slid without
/// changing its type. Dragging (or clicking) the rail moves the value.
/// The knob position is derived from it with the same two-`Fill`-leaf
/// trick [`crate::ProgressBar`] uses — `Fill(fraction)` left of the knob,
/// `Fill(1 − fraction)` right — so it tracks the resolved width without
/// the widget knowing it at record time. Pointer→value mapping uses last
/// frame's arranged width (one-frame lag, invisible at interactive
/// rates). Visuals come from [`crate::SliderTheme`] (theme slot
/// `slider`).
#[derive(Debug)]
pub struct Slider<'a> {
    node: Node,
    value: DragNum<'a>,
    min: f64,
    max: f64,
    step: Option<f64>,
    decimals: usize,
    style: Option<&'a SliderTheme>,
}

impl<'a> Slider<'a> {
    /// The range is a constructor argument rather than the builder
    /// [`DragValue::range`](crate::DragValue::range): a slider maps a
    /// rail position onto its bounds, so it has no meaning without them,
    /// where an unbounded scrub is the drag value's default.
    #[track_caller]
    pub fn new(value: impl Into<DragNum<'a>>, range: RangeInclusive<f64>) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK | Sense::DRAG);
        Self {
            node,
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
    pub fn step(mut self, step: f64) -> Self {
        assert!(
            step.is_finite() && step > 0.0,
            "slider step must be finite and greater than zero, got {step}",
        );
        self.step = Some(step);
        self
    }

    /// Digits after the decimal point a float target's committed value
    /// keeps, so a drag never stores a long tail. Ignored by the integer
    /// target, which rounds whole. Default `2`, matching
    /// [`DragValue::decimals`](crate::DragValue::decimals).
    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    style_setter!('a, SliderTheme, slider);

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
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        let theme = self.slot(ui.theme());
        let knob = theme.knob_size;
        let rail_h = theme.rail_thickness;
        let fill_color = theme.fill;
        let rail_color = theme.rail;
        let knob_color = theme.knob;

        // Pointer drives the value: pressing or dragging the rail maps
        // the cursor x against the last frame's logical width.
        //
        // `Drag::Stopped` is neither pressed nor dragging, so the release
        // frame has to be named for `ValueResponse::committed` to mean what
        // it documents. Replaying the value there needs no retained `last`
        // the way `DragValue` does: it is a function of the pointer, not of
        // accumulated travel.
        let stopped = response.left.drag.stopped();
        let mut changed = false;
        if !response.disabled
            && (response.pressed() || response.left.drag.dragging() || stopped)
            && let (Some(local), Some(rect)) = (response.pointer_local, response.layout_rect)
        {
            let f = pointer_to_fraction(local.x, rect.size.w, knob);
            let v = snap_to_step(
                fraction_to_value(f, self.min, self.max),
                self.min,
                self.step,
            );
            changed = self.value.commit_drag(v, self.decimals, self.min, self.max);
        }
        // Edge, not level: the frame the gesture ends is the one a
        // caller treats as a single undoable edit.
        let committed = !response.disabled && stopped;
        let fraction = value_to_fraction(self.value.get(), self.min, self.max);

        let pill = Corners::all(rail_h * 0.5);
        let fill_bg = Background::rounded(fill_color, pill);
        let rail_bg = Background::rounded(rail_color, pill);
        let knob_bg = Background::rounded(knob_color, Corners::all(knob * 0.5));

        let node = &mut widget.node;
        node.size
            .get_or_insert((Sizing::FILL, Sizing::fixed(knob)).into());
        node.child_align = Align::v(VAlign::Center);

        // The knob sits between two rails whose weights partition the
        // track, so its position tracks the resolved width without this
        // widget knowing that width at record time.
        let [filled, remainder] = Sizing::split(fraction);
        widget.record(ui, None, |ui| {
            let rail = Sizing::fixed(rail_h);
            ui.chrome_leaf(id.with("fill"), (filled, rail), Some(&fill_bg));
            let knob = Sizing::fixed(knob);
            ui.chrome_leaf(id.with("knob"), (knob, knob), Some(&knob_bg));
            ui.chrome_leaf(id.with("rail"), (remainder, rail), Some(&rail_bg));
        });
        ValueResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl_configure!(Slider<'_>);

/// Fraction (0..1) of the way from `min` to `max` that `value` sits.
///
/// The knob's position is a visual quantity, so the share is taken in
/// `f32` — through [`approx::ratio`], the crate's one answer for a
/// divisor geometry can legitimately collapse, and out through the same
/// [`F32Ext::unit_fraction_or`] [`pointer_to_fraction`] ends in. Both of
/// this widget's fractions are then total and answer a range or a value
/// that names no share the same way: the low end.
fn value_to_fraction(value: f64, min: f64, max: f64) -> f32 {
    approx::ratio((value - min) as f32, (max - min) as f32).unit_fraction_or(0.0)
}

/// Inverse of [`value_to_fraction`]: the value at `fraction` of the
/// range. Taken in `f64` so a wide range keeps the precision the bound
/// value is stored at.
fn fraction_to_value(fraction: f32, min: f64, max: f64) -> f64 {
    min + f64::from(fraction.clamp(0.0, 1.0)) * (max - min)
}

/// Map a cursor x (relative to the rail's left edge) to a fraction. The
/// usable travel is `[knob/2, track_w - knob/2]` so the knob center
/// stays inside the rail at both extremes. A rail with no travel reads
/// as the low end, and so does a cursor that names no position.
fn pointer_to_fraction(local_x: f32, track_w: f32, knob: f32) -> f32 {
    local_x.band_fraction(track_w, knob).unit_fraction_or(0.0)
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
