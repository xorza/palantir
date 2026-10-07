//! The per-window settings the host applies after a frame.

use crate::primitives::geometry::rect::Rect;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;

/// Per-window levels a recorder re-reads every frame and a host applies, unlike the one-shot edges in [`WindowCommands`](crate::window::window_commands::WindowCommands).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct WindowOutput {
    pub(crate) cursor: CursorIcon,
    pub(crate) vsync: Vsync,
    /// The IME caret in logical px, or `None` for off; a pass nobody asks in turns it off.
    pub(crate) ime: Option<Rect>,
}
