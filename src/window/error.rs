//! [`ScreenshotRefusal`]: why [`Ui::request_screenshot`](crate::Ui::request_screenshot) took no
//! ask.

use crate::window::window_requests::MAX_PENDING_SCREENSHOTS;
use std::error;
use std::fmt;

/// Why a window took no screenshot ask; no image lands for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenshotRefusal {
    /// The region holds no pixel of the window.
    EmptyRegion,
    /// The window owes as many images as it holds at once: asked and not painted, or on their way
    /// back from the device. One lands a frame, so an ask in a later frame finds room.
    Full,
}

impl fmt::Display for ScreenshotRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRegion => f.write_str("the region holds no pixel of the window"),
            Self::Full => write!(
                f,
                "the window owes {MAX_PENDING_SCREENSHOTS} screenshots already; one lands a frame"
            ),
        }
    }
}

impl error::Error for ScreenshotRefusal {}
