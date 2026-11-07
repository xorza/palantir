//! The per-window, per-frame hook a host calls. It lives in `ui`, beside the frame cycle that calls
//! it, as winit's `ApplicationHandler` sits beside its event loop and eframe's `App` beside its run
//! loop. Refused: a top-level `app` module for the contract alone, which made a cycle with `ui`.

use crate::ui::Ui;
use crate::window::window_token::WindowToken;

/// Which record pass of its frame [`App::record`] runs, as [`Ui::record_pass`] reads it, so work
/// that must not replay runs in one pass: what the host or a worker handed over, and what a click or
/// a key means, read at the top of the record against the rects of the last frame.
///
/// A frame runs one to three passes, in this order, and each opens with the same `Ui`, as egui tells
/// its passes apart with
/// [`Context::current_pass_index`](https://docs.rs/egui/latest/egui/struct.Context.html#method.current_pass_index).
/// Refused: `App::update`, a hook ahead of the record with a view of its own and an answer on what
/// the record may read, which wrote each read twice and kept a phase order a first frame broke.
/// Refused too: a pass index, which leaves the warmup a number each app must know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecordPass {
    /// A cold window's first frame only, and its output is discarded: no frame is on screen, so it
    /// reads no rects and no input. An app sets up here what a record needs.
    Warmup,
    /// Every recorded frame's pass that reads the frame's input.
    #[default]
    First,
    /// A pass past [`Self::First`], when a click, a key press, a drag latch or a drag stop came, or
    /// the first asked with [`Ui::request_relayout`]. The edges are gone, and it reads the rects of
    /// the pass before it, so a write in the first shows here.
    Second,
}

/// Application lifecycle driven by a host for each window that records.
///
/// Refused: retained handlers (`on_click` closures or messages, as in iced, GPUI and Flutter). Their
/// order is the same: an event meets the last layout, the state changes, then the build runs. In
/// palantir they allocate per frame and add a second way for each widget. An id and
/// [`Ui::response_for`] give the same order, as egui's `read_response`
/// does.
///
/// Every window records from the one app, and a frame of any of them may change it, so the host
/// records each other shown window after it: a value one window edits shows in all of them, with
/// no wake to send. A hidden window records when it shows again. The frame a window records only
/// to follow another's asks no window to follow it in turn, so a change a record pass makes with
/// no input, wake or animation of its own shows in the other windows at their next frame.
pub trait App {
    /// Build the UI for `window`. Runs once per [`RecordPass`] of the frame, so work that must not
    /// replay reads [`Ui::record_pass`] first.
    fn record(&mut self, window: WindowToken, ui: &mut Ui);

    /// Whether work the app waits on runs off the frame and will [wake](crate::Waker) the window
    /// when it lands: a conversion on a worker thread, a file that loads. A settle waits it out:
    /// a [`HarnessSession`](crate::HarnessSession)'s settle and `Remote::settle` count a busy app as not
    /// settled. Read after a frame. Default `false`.
    fn is_busy(&self) -> bool {
        false
    }
}

/// A closure is the app of a host with one window: it records every window it is asked for.
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

    /// A closure records once for each window a host asks, whatever its token.
    #[test]
    fn a_closure_records_every_window() {
        let mut h = Harness::mono(UVec2::ONE);
        let mut records = 0;
        let mut app = |_: &mut Ui| records += 1;
        app.record(WindowToken(0), h.ui_mut());
        app.record(WindowToken(7), h.ui_mut());
        assert_eq!(records, 2);
    }
}
