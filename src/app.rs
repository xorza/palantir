use crate::ui::Ui;
use crate::window::WindowToken;

/// Application lifecycle driven by a host for each window that records.
pub trait App {
    /// Run once before the first record pass of a fully recorded frame for
    /// `win`. Use this for unconditional application mutation, queue drains,
    /// task submission, telemetry, and other work that must not replay.
    /// `ui` exposes the current frame's display, clock, and unsuppressed input,
    /// but is read-only so this phase cannot accidentally emit widgets.
    /// Paint-only animation frames reuse the retained tree and skip both app
    /// hooks.
    fn update(&mut self, _win: WindowToken, _ui: &Ui) {}

    /// Build the UI for window `win`. This hook may replay for cold-start
    /// warmup, action input, or `Ui::request_relayout`; unconditional external
    /// effects belong in [`Self::update`]. Switch on `win` to drive different
    /// windows; open or close further windows via [`Ui::open_window`] and
    /// [`Ui::close_window`].
    fn record(&mut self, win: WindowToken, ui: &mut Ui);
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::app::App;
    use crate::ui::Ui;
    use crate::window::WindowToken;

    #[derive(Debug)]
    pub(crate) struct RecordApp<F> {
        record: F,
    }

    impl<F: FnMut(&mut Ui)> RecordApp<F> {
        pub(crate) fn new(record: F) -> Self {
            Self { record }
        }
    }

    impl<F: FnMut(&mut Ui)> App for RecordApp<F> {
        fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
            (self.record)(ui);
        }
    }
}
