//! The per-window settings the host applies after a frame.

use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;

/// What the host applies after draining a frame's recorder output.
#[cfg_attr(
    not(feature = "winit-host"),
    expect(
        dead_code,
        reason = "multi-window lifecycle plumbing: every caller is under \
                  src/host/winit/, so a build without that feature has \
                  nothing to call it"
    )
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowOutput {
    /// The cursor this frame asked for; applied on change.
    pub(crate) cursor: CursorIcon,
    /// The pacing this frame wants. A level: the host diffs it against the
    /// swapchain it has open and reconfigures only on a change.
    pub(crate) vsync: Vsync,
}
