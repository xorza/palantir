//! [`DoubleClickWindow`]: how soon a second press makes a double click, one value a host shares
//! with every window's input.

use crate::input_state::capture::DOUBLE_CLICK_WINDOW;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// The double-click time every window of a host reads at each press: the platform's, when the
/// host reads one, else [`DOUBLE_CLICK_WINDOW`]. A clone shares the value, so the host sets it
/// once for every window.
#[derive(Clone, Debug)]
pub(crate) struct DoubleClickWindow(Rc<Cell<Duration>>);

impl Default for DoubleClickWindow {
    fn default() -> Self {
        Self(Rc::new(Cell::new(DOUBLE_CLICK_WINDOW)))
    }
}

impl DoubleClickWindow {
    pub(crate) fn get(&self) -> Duration {
        self.0.get()
    }

    #[cfg(any(test, feature = "winit"))]
    pub(crate) fn set(&self, window: Duration) {
        self.0.set(window);
    }
}
