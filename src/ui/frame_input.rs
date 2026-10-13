//! [`FrameInput`]: the bundle a window driver hands `Ui::frame`.

use crate::display::frame_stamp::FrameStamp;
use crate::window::window_token::WindowToken;

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
