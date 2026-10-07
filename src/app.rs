//! The per-window, per-frame hook a host calls, and the closure shorthand for record-only apps.

use crate::ui::Ui;
use crate::window::window_token::WindowToken;

/// Application lifecycle driven by a host for each window that records.
pub trait App {
    /// Runs once before the first record pass of a fully recorded frame, for work that must not replay. `ui` is read-only.
    fn update(&mut self, _window: WindowToken, _ui: &Ui) {}

    /// Build the UI for `window`. May replay (warmup, action input, relayout) with no action input after the first pass, so unconditional effects belong in [`Self::update`].
    fn record(&mut self, window: WindowToken, ui: &mut Ui);
}
