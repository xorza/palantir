//! The per-frame monotonic time and display, and the bundle a window driver hands `Ui::frame`.

use crate::display::Display;
use crate::window::window_token::WindowToken;
use std::time::Duration;

/// Per-frame window-driver inputs: monotonic `time` and the active [`Display`]. One struct so `Ui` retains one `Option<FrameStamp>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameStamp {
    pub(super) display: Display,
    pub(super) time: Duration,
}

impl FrameStamp {
    pub(crate) const fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}

/// What a window driver hands `Ui::frame`: the stamp, whether last frame's damage snapshot still describes the surface (`false` forces a full repaint), and the window the app records for. Separate from [`FrameStamp`] because only the stamp is retained (`FrameRuntime::prev_stamp`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInput {
    pub(super) stamp: FrameStamp,
    pub(super) damage_baseline_valid: bool,
    pub(super) window: WindowToken,
}

impl FrameInput {
    pub(crate) const fn new(
        stamp: FrameStamp,
        damage_baseline_valid: bool,
        window: WindowToken,
    ) -> Self {
        Self {
            stamp,
            damage_baseline_valid,
            window,
        }
    }
}
