//! The per-window, per-frame hook a host calls. It lives in `ui`, beside the frame cycle that calls
//! it, as winit's `ApplicationHandler` sits beside its event loop and eframe's `App` beside its run
//! loop. Refused: a top-level `app` module for the contract alone, which made a cycle with `ui`.

use crate::ui::Ui;
use crate::ui::update_ctx::UpdateCtx;
use crate::window::window_token::WindowToken;

/// What [`App::update`] did with this frame's input, which decides whether a record may read it.
///
/// A frame-wide answer, as a frame holds little: `InputQueue` admits one press or release per
/// button and one command key. Event systems claim per event, as Flutter's
/// [`KeyEventResult`](https://api.flutter.dev/flutter/widgets/KeyEventResult.html) and egui's
/// `consume_key` do, where events reach handlers one by one. Refused: a policy that keeps the edges
/// and records once, with a contract that a record asks for a second pass after each write, which
/// paints a stale frame where a widget forgets and which every custom widget must learn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub enum InputOutcome {
    /// A record reads this frame's input, and a frame with a click, a key press, a drag latch or a
    /// drag stop records again, so a write in the first pass shows in the second.
    #[default]
    Record,
    /// This hook acted on all of this frame's input: no record reads an edge of it, not a click, a
    /// key, a drag edge or a wheel's delta, so no record can react, and the frame records once
    /// unless a record asks. Levels stay, as a held button and the pointer's position. Returned for
    /// input a widget reads, it takes that input away from the widget.
    Handled,
}

/// Application lifecycle driven by a host for each window that records.
///
/// Refused: retained handlers (`on_click` closures or messages, as in iced, GPUI and Flutter). Their
/// order is the same: an event meets the last layout, the state changes, then the build runs. In
/// palantir they allocate per frame and add a second way for each widget. An id and
/// [`Input::response_for`](crate::Input::response_for) give the same order, as egui's `read_response`
/// does. Refused too: `App::input` beside [`Self::update`], split only by a parameter type, and
/// `App::setup` ahead of the warmup, a hook for frame 1's sake.
///
/// Every window records from the one app, and a frame of any of them may change it, so the host
/// records each other shown window after it: a value one window edits shows in all of them, with
/// no wake to send. A hidden window records when it shows again. The frame a window records only
/// to follow another's asks no window to follow it in turn, so a change a record pass makes with
/// no input, wake or animation of its own shows in the other windows at their next frame.
pub trait App {
    /// Runs once a fully recorded frame, before any record, the first frame's warmup included, for
    /// work that must not replay: setup, what the host or a worker handed over, and this frame's
    /// input, its clicks, its keys and its drags, against the rects of the last frame. A state this
    /// writes shows in the first record pass. An [`UpdateCtx`] holds no [`Ui`], so nothing can record
    /// here, as [`eframe`](https://docs.rs/eframe/latest/eframe/trait.App.html)'s `logic` runs before
    /// its `ui`.
    ///
    /// Before the first warmup no frame is on screen, so this reads no rects and no click there. A
    /// Tab pressed before the window opened counts only over a focus set by then, and it moves that
    /// focus in the first pass, past this read.
    ///
    /// Returns what the frame's records may read of the input; [`InputOutcome::Record`] by default.
    fn update(&mut self, _window: WindowToken, _ctx: UpdateCtx<'_>) -> InputOutcome {
        InputOutcome::Record
    }

    /// Build the UI for `window`. May replay (warmup, action input, relayout) with no action input after the first pass, so unconditional effects belong in [`Self::update`].
    fn record(&mut self, window: WindowToken, ui: &mut Ui);

    /// Whether work the app waits on runs off the frame and will [wake](crate::Waker) the window
    /// when it lands: a conversion on a worker thread, a file that loads. A settle waits it out:
    /// a [`HarnessSession`](crate::HarnessSession)'s settle and `Remote::settle` count a busy app as not
    /// settled. Read after a frame. Default `false`.
    fn is_busy(&self) -> bool {
        false
    }
}

/// A closure is the app of a host with one window: it records every window it is asked for, and
/// updates nothing.
impl<F: FnMut(&mut Ui)> App for F {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        self(ui);
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::ui::Ui;
    use crate::ui::app::App;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    /// A closure records once for each window a host asks, whatever its token, and its update
    /// does nothing.
    #[test]
    fn a_closure_records_every_window() {
        let mut h = Harness::mono(UVec2::ONE);
        let mut records = 0;
        let mut app = |_: &mut Ui| records += 1;
        let _ = app.update(WindowToken(0), h.ui_mut().update_ctx());
        app.record(WindowToken(0), h.ui_mut());
        app.record(WindowToken(7), h.ui_mut());
        assert_eq!(records, 2);
    }
}
