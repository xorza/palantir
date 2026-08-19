//! The per-frame monotonic time and active display.

use crate::display::Display;
use std::time::Duration;

/// WindowDriver-supplied per-frame inputs — monotonic time + active
/// [`Display`]. Single struct so callers pass one argument and
/// `Ui` carries one `Option<FrameStamp>` for prior-frame state
/// instead of two parallel fields. `time` is the host's monotonic
/// clock (driven by the same source between frames); `display`
/// carries the surface size + scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameStamp {
    pub(super) display: Display,
    pub(super) time: Duration,
}

impl FrameStamp {
    pub(crate) fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}
