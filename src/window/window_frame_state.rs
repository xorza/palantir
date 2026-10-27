//! What a window's host hands its `Ui` for one frame.

use crate::window::screenshot::Screenshot;
use crate::window::window_placement::WindowPlacement;

/// What the host hands `Ui` for a frame: the window manager's facts, cleared when the frame's
/// output drains, and the screenshot that landed for it, which the app may read until the next
/// frame replaces it.
#[derive(Debug, Default)]
pub(crate) struct WindowFrameState {
    pub(crate) close_requested: bool,
    pub(crate) placement: WindowPlacement,
    /// The image that landed for this frame, if one did. Boxed: a rare, cold value kept out of
    /// the fields every pass walks (`hot_struct_sizes`).
    pub(crate) screenshot: Option<Box<Screenshot>>,
}

impl WindowFrameState {
    /// Clear the facts once the frame's output drained, so a host that publishes none reads as
    /// a window nobody is closing.
    pub(crate) fn clear_facts(&mut self) {
        self.close_requested = false;
        self.placement = WindowPlacement::default();
    }
}

/// The window manager's facts a host publishes before a frame.
#[cfg(feature = "winit")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowFacts {
    pub(crate) close_requested: bool,
    pub(crate) placement: WindowPlacement,
}
