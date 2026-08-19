//! What a window driver hands `Ui::frame` on entry.

use crate::ui::frame_stamp::FrameStamp;

/// What a window driver hands `Ui::frame` on entry: the frame's stamp,
/// plus whether last frame's damage snapshot still describes the surface
/// — `false` forces a full repaint instead of a partial one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInput {
    pub(crate) stamp: FrameStamp,
    pub(crate) damage_baseline_valid: bool,
}
