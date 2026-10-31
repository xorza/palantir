//! What the whole tour shares, threaded into every page beside the `Ui`.

use palantir::RgbaF32;

use crate::appearance::Appearance;
use crate::job::Job;

/// The app's state: the host threads `&mut AppState` into the record call beside `&mut Ui`, so
/// any page, and the inspector window, reads and writes it without a global.
#[derive(Debug)]
pub(crate) struct AppState {
    /// The values the windows page and the inspector share.
    pub(crate) counter: i32,
    pub(crate) note: String,
    pub(crate) color: RgbaF32,
    pub(crate) appearance: Appearance,
    /// The windows page's background job, while one runs or since it finished.
    pub(crate) job: Option<Job>,
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            counter: 0,
            note: String::from("shared between both windows"),
            color: RgbaF32::hex(0x3b82f6),
            appearance: Appearance::new(),
            job: None,
        }
    }
}
