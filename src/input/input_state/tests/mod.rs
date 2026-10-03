mod click;
mod drag;
mod input_delta;
mod keyboard;
mod response_state;
mod scroll;
mod scroll_routing;
mod settle;
mod trickle;
mod watch;
mod zoom;

use crate::Ui;
use crate::cascade::Cascade;
use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::interaction::input_delta::InputDelta;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::{button::Button, panel::Panel};
use glam::UVec2;
use std::time::Duration;
use strum::EnumCount as _;

impl InputState {
    /// Feed `event` at time zero against an empty cascade — the input
    /// machine alone, with no tree behind it.
    fn feed(&mut self, event: InputEvent) -> InputDelta {
        self.on_input(event, &Cascade::default(), Duration::ZERO)
    }
}

/// The surface the button scenes run on.
const BUTTON_SURFACE: UVec2 = UVec2::new(200, 80);

/// A 100×40 button: the target most input tests press, focus or probe.
fn fixed_button<'a>(id: WidgetId) -> Button<'a> {
    Button::new()
        .id(id)
        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
}

/// A 100×40 "hi" [`fixed_button`] at the start of an auto-id row.
fn build_button(id: WidgetId) -> impl Fn(&mut Ui) + Copy {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(id).label("hi").show(ui);
        });
    }
}

/// Which per-layer event queue [`sample_layers`] reads.
#[derive(Clone, Copy, Debug)]
enum Stream {
    Keyboard,
    Pointer,
}

/// What [`sample_layers`] read in pass A: the per-layer event counts of
/// one stream, and the record closure's own value.
#[derive(Debug)]
struct Sample<R> {
    layers: [usize; Layer::COUNT],
    value: R,
}

/// Per-layer event counts of `stream`, read *inside* pass A's record —
/// the only place they are live, since `end_frame` drains the queues and
/// pass B sees them drained.
fn sample_layers<R>(
    h: &mut UiHarness,
    stream: Stream,
    mut record: impl FnMut(&mut Ui) -> R,
) -> Sample<R> {
    h.frame_value(|ui| {
        let value = record(ui);
        let input = ui.input();
        Sample {
            // `PAINT_ORDER[i]` is the layer whose `idx()` is `i`.
            layers: Layer::PAINT_ORDER.map(|layer| match stream {
                Stream::Keyboard => input.keyboard_events(layer).len(),
                Stream::Pointer => input.pointer_events(layer).len(),
            }),
            value,
        }
    })
}

/// A focus holder no widget records — what a test forges to stand in for
/// a prior grant, such as a text editor clicked last frame.
fn forged_focus() -> WidgetId {
    WidgetId::from_hash("forged-focus")
}
