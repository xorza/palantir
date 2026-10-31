//! What the whole tour shares, threaded into every page beside the `Ui`.

use crate::appearance::Appearance;

/// The app's state: the host threads `&mut AppState` into the record call beside `&mut Ui`, so
/// any page, and the inspector window, reads and writes it without a global.
#[derive(Debug)]
pub(crate) struct AppState {
    /// The value the windows page and the inspector share.
    pub(crate) counter: i32,
    pub(crate) appearance: Appearance,
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            counter: 0,
            appearance: Appearance::new(),
        }
    }
}
