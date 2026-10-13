//! [`FrameStamp`]: the display a frame presents under, and when.

use crate::display::Display;
use std::time::Duration;

/// Per-frame window-driver inputs: monotonic `time` and the active [`Display`]. One struct so `Ui` retains one `Option<FrameStamp>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameStamp {
    pub(crate) display: Display,
    pub(crate) time: Duration,
}

impl FrameStamp {
    pub(crate) const fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}
