//! The per-window settings the host applies after a frame.

use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;

/// The per-window *levels* a recorder holds and a host applies: settings
/// it re-reads every frame, as opposed to the one-shot lifecycle edges in
/// [`WindowCommands`](crate::window::window_commands::WindowCommands).
///
/// Retained on [`WindowRequests`](crate::window::window_requests::WindowRequests)
/// and copied out by each drain, which is what lets a host with no window
/// to apply them to drop its copy without the app's own view of them
/// changing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowOutput {
    /// The cursor this frame asked for; applied on change.
    pub(crate) cursor: CursorIcon,
    /// The pacing this frame wants. A level: the host diffs it against the
    /// swapchain it has open and reconfigures only on a change.
    pub(crate) vsync: Vsync,
}
