//! The horizontal value slider, and what a frame of it reports about the
//! value it writes through.

use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::limits::Limits;
use crate::primitives::num::F32Ext;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::slider::SliderTheme;
use std::ops::RangeInclusive;

/// What [`Slider::show`] reports about the value it writes through.
#[derive(Debug)]
pub struct SliderResponse<'a> {
    /// The widget's pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The drag moved the bound value this frame. A **level** while the
    /// pointer is dragging it somewhere new, and false on a drag that
    /// is pinned at an end of the range.
    pub changed: bool,
    /// The drag released this frame, so the bound value holds its final
    /// result — one gesture, one undoable edit.
    ///
    /// The release frame **re-writes** that value, so a caller that ignores
    /// `changed`, re-seeds the bound `f32` from its own canonical copy every
    /// frame, and adopts it only here still observes what the gesture landed
    /// on. Released while disabled, the gesture is dropped instead.
    pub committed: bool,
}

/// Horizontal value slider over a `f32` range. Takes a `&mut f32`;
/// dragging (or clicking) the rail moves the value. The knob position is
/// derived from the value with the same two-`Fill`-leaf trick as
/// [`crate::ProgressBar`] — `Fill(fraction)` left of the knob,
/// `Fill(1 − fraction)` right — so it tracks the resolved width without
/// the widget knowing it at record time. Pointer→value mapping uses last
/// frame's arranged width (one-frame lag, invisible at interactive
/// rates). Visuals come from [`crate::SliderTheme`] (theme slot
/// `slider`).
#[derive(Debug)]
pub struct Slider<'a> {
    node: Node,
    value: &'a mut f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    style: Option<&'a SliderTheme>,
}

impl<'a> Slider<'a> {
    #[track_caller]
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK | Sense::DRAG);
        Self {
            node,
            value,
            min: *range.start(),
            max: *range.end(),
            step: None,
            style: None,
        }
    }

    /// Snap the value to multiples of `step` (anchored at `min`). `0` or
    /// negative disables snapping (the default — continuous).
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    style_setter!('a, SliderTheme, slider);

    /// Record the slider and report what the drag did to the bound
    /// value.
    ///
    /// A [`SliderResponse`] rather than a bare [`Response`] because the
    /// widget writes through `&mut f32`: the caller has no other way to
    /// tell whether the value moved this frame. `left.drag.dragging()`
    /// does not answer it — a drag pinned at `min`/`max` keeps reporting
    /// while the value stays put. Same level/edge split as
    /// [`DragValueResponse`](crate::DragValueResponse), so the two
    /// value-editing widgets read alike.
    pub fn show(self, ui: &mut Ui) -> SliderResponse<'_> {
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
        // frame has to be named for `SliderResponse::committed` to mean what
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
            let next = Limits::of(self.min, self.max).clamp(v);
            changed = next != *self.value;
            *self.value = next;
        }
        // Edge, not level: the frame the gesture ends is the one a
        // caller treats as a single undoable edit.
        let committed = !response.disabled && stopped;
        let fraction = value_to_fraction(*self.value, self.min, self.max);

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
        SliderResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl_configure!(Slider<'_>);

/// Fraction (0..1) of the way from `min` to `max` that `value` sits.
/// Degenerate (`min == max`) ranges map to 0.
fn value_to_fraction(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if approx::approx_zero(span) {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

/// Inverse of [`value_to_fraction`]: the value at `fraction` of the
/// range.
fn fraction_to_value(fraction: f32, min: f32, max: f32) -> f32 {
    min + fraction.clamp(0.0, 1.0) * (max - min)
}

/// Map a cursor x (relative to the rail's left edge) to a fraction. The
/// usable travel is `[knob/2, track_w - knob/2]` so the knob center
/// stays inside the rail at both extremes. A rail with no travel reads
/// as the low end, and so does a cursor that names no position.
fn pointer_to_fraction(local_x: f32, track_w: f32, knob: f32) -> f32 {
    local_x.band_fraction(track_w, knob).unit_fraction_or(0.0)
}

/// Snap to the nearest multiple of `step` measured from `min`. A `None`
/// or non-positive step is a passthrough.
fn snap_to_step(value: f32, min: f32, step: Option<f32>) -> f32 {
    match step {
        Some(s) if s > 0.0 => min + ((value - min) / s).round() * s,
        _ => value,
    }
}

#[cfg(test)]
mod tests;
