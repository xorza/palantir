mod click;
mod drag;
mod ime;
mod input_delta;
mod keyboard;
mod response_state;
mod scroll;
mod scroll_routing;
mod settle;
mod tab;
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

impl InputState {
    /// Feed `event` at time zero against an empty cascade: the input machine alone.
    fn feed(&mut self, event: InputEvent<'_>) -> InputDelta {
        self.on_input(event, &Cascade::default(), Duration::ZERO)
    }
}

const BUTTON_SURFACE: UVec2 = UVec2::new(200, 80);

fn fixed_button<'a>(id: WidgetId) -> Button<'a> {
    Button::new()
        .id(id)
        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
}

fn build_button(id: WidgetId) -> impl Fn(&mut Ui) + Copy {
    move |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            fixed_button(id).label("hi").show(ui);
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum Stream {
    Keyboard,
    Pointer,
}

#[derive(Debug)]
struct Sample<R> {
    layers: [usize; Layer::COUNT],
    value: R,
}

/// Per-layer event counts of `stream`, read inside pass A's record, the only place they are live (`end_frame` drains the queues).
fn sample_layers<R>(
    h: &mut UiHarness,
    stream: Stream,
    mut record: impl FnMut(&mut Ui) -> R,
) -> Sample<R> {
    h.frame_value(|ui| {
        let value = record(ui);
        let input = ui.input();
        Sample {
            layers: Layer::PAINT_ORDER.map(|layer| match stream {
                Stream::Keyboard => input.keyboard_events(layer).len(),
                Stream::Pointer => input.pointer_events(layer).len(),
            }),
            value,
        }
    })
}

/// A focus holder no widget records, standing in for a prior grant such as a text editor clicked last frame.
fn forged_focus() -> WidgetId {
    WidgetId::from_hash("forged-focus")
}
