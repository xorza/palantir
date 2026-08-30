//! The per-frame monotonic time and active display, and the bundle a window
//! driver hands them to `Ui::frame` in.

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

/// What a window driver hands `Ui::frame` on entry: the frame's stamp,
/// plus whether last frame's damage snapshot still describes the surface
/// — `false` forces a full repaint instead of a partial one.
///
/// A wrapper rather than two more fields on [`FrameStamp`], because only
/// the stamp is retained: `FrameRuntime::prev_stamp` keeps one across
/// frames, while the damage flag answers for the frame it arrives on and
/// no other.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInput {
    pub(super) stamp: FrameStamp,
    pub(super) damage_baseline_valid: bool,
}

impl FrameInput {
    pub(crate) fn new(stamp: FrameStamp, damage_baseline_valid: bool) -> Self {
        Self {
            stamp,
            damage_baseline_valid,
        }
    }
}
