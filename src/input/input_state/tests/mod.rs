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

use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::response::input_delta::InputDelta;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use std::time::Duration;

/// A first press of `key`, typing what the key types on a plain layout.
fn key_down(key: Key) -> InputEvent {
    InputEvent::KeyDown {
        key,
        repeat: false,
        physical: Key::Other,
        text: KeyText::of_key(key),
    }
}

impl InputState {
    /// Feed `event` at time zero against an empty cascade — the input
    /// machine alone, with no tree behind it.
    fn feed(&mut self, event: InputEvent) -> InputDelta {
        self.on_input(event, &Cascade::default(), Duration::ZERO)
    }
}

/// A focus holder no widget records — what a test forges to stand in for
/// a prior grant, such as a text editor clicked last frame.
fn forged_focus() -> WidgetId {
    WidgetId::from_hash("forged-focus")
}
