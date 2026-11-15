mod capture;
mod click;
mod drag;
mod ime;
mod input_delta;
mod key_states;
mod keyboard;
mod response_state;
mod scroll;
mod scroll_routing;
mod settle;
mod tab;
mod trickle;
mod watch;
mod zoom;

use crate::host::harness::Harness;
use crate::input::Input;
use crate::input::capture::DOUBLE_CLICK_INTERVAL;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::input_delta::InputDelta;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::{button::Button, panel::Panel};
use glam::UVec2;
use std::time::Duration;

/// `widget` as an overlay's claim on its layer, as `OverlayScope::claim` stamps a root: a
/// scope taking every key, which cuts the planes below off.
fn claiming<W: Configure>(mut widget: W) -> W {
    widget
        .configure()
        .input_scope(KeyFilter::ALL)
        .blocks_below();
    widget
}

impl Input {
    /// Feed `event` at time zero against this input's arranged frame.
    fn feed(&mut self, event: InputEvent<'_>) -> InputDelta {
        self.on_input(event, DOUBLE_CLICK_INTERVAL, Duration::ZERO)
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

/// Per-layer event counts of `stream`, read inside pass A's record, the only place they are live (`Input::settle` clears the edges).
fn sample_layers<R>(
    h: &mut Harness,
    stream: Stream,
    mut record: impl FnMut(&mut Ui) -> R,
) -> Sample<R> {
    h.frame_value(|ui| {
        let value = record(ui);
        let input = ui.input();
        Sample {
            layers: Layer::PAINT_ORDER.map(|layer| match stream {
                Stream::Keyboard => input.keyboard_events_at(layer.into()).len(),
                Stream::Pointer => input.pointer_events_at(layer.into()).len(),
            }),
            value,
        }
    })
}

/// A focus holder no widget records, standing in for a prior grant such as a text editor clicked last frame.
fn forged_focus() -> WidgetId {
    WidgetId::from_hash("forged-focus")
}
