//! The per-window, per-frame hook a host calls.

use crate::ui::Ui;
use crate::ui::input_ctx::InputCtx;
use crate::window::window_token::WindowToken;

/// Application lifecycle driven by a host for each window that records.
///
/// Every window records from the one app, and a frame of any of them may change it, so the host
/// records each other shown window after it: a value one window edits shows in all of them, with
/// no wake to send. A hidden window records when it shows again. The frame a window records only
/// to follow another's asks no window to follow it in turn, so a change a record pass makes with
/// no input, wake or animation of its own shows in the other windows at their next frame.
pub trait App {
    /// Runs once before any record of a fully recorded frame, the first frame's warmup included,
    /// for work that must not replay and is not input. `ui` is read-only, and reads what the first
    /// record pass reads: focus has already moved for this frame's Tab and arrow presses.
    fn update(&mut self, _window: WindowToken, _ui: &Ui) {}

    /// Runs once a fully recorded frame, after [`Self::update`] and before any record, with this
    /// frame's input: its clicks, its keys and its drags, against the rects of the last frame. A
    /// state this writes shows in the first record pass. An [`InputCtx`] holds no [`Ui`], so nothing
    /// can record here, as [`eframe`](https://docs.rs/eframe/latest/eframe/trait.App.html)'s `logic`
    /// runs before its `ui`. Refused: one hook with both, as a `&Ui` and an [`InputCtx`] cannot exist
    /// at one time.
    fn input(&mut self, _window: WindowToken, _ctx: InputCtx<'_>) {}

    /// Build the UI for `window`. May replay (warmup, action input, relayout) with no action input after the first pass, so unconditional effects belong in [`Self::update`] or [`Self::input`].
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
        app.update(WindowToken(0), h.ui());
        app.record(WindowToken(0), h.ui_mut());
        app.record(WindowToken(7), h.ui_mut());
        assert_eq!(records, 2);
    }
}
