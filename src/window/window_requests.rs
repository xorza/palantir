//! Everything a frame's recorder asks of its window host.

use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;

/// Deferred recorder output consumed by the window host after a frame.
#[derive(Debug, Default)]
pub(crate) struct WindowRequests {
    pub(crate) commands: WindowCommands,
    /// Whether app code vetoed the current close request.
    pub(crate) close_vetoed: bool,
    /// Last cursor requested during a record pass; retained across PaintOnly.
    pub(crate) cursor: CursorIcon,
    /// The presentation pacing this window is currently set to — a level
    /// like `cursor`, not an edge. Seeded from the swapchain the host
    /// actually opened, so it answers [`Ui::set_vsync`](crate::Ui::set_vsync)
    /// truthfully from the first frame, and retained across frames so a
    /// recorder never has to re-assert it. Reconfiguring a swapchain is
    /// expensive, so the *host* compares this against the mode in force and
    /// acts only on a real flip.
    pub(crate) vsync: Vsync,
}
