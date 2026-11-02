//! What a window's host hands its `Ui` for one frame.

use crate::window::screenshot::Screenshot;
use crate::window::window_placement::WindowPlacement;
use glam::UVec2;

/// What the host hands `Ui` for a frame: the window manager's facts, cleared when the frame's
/// output drains, and the screenshot that landed for it, which the app may read until the next
/// frame replaces it.
#[derive(Debug, Default)]
pub(crate) struct WindowFrameState {
    pub(crate) facts: WindowFacts,
    /// The image that landed for this frame, if one did. Boxed: a rare, cold value kept out of
    /// the fields every pass walks (`hot_struct_sizes`).
    pub(crate) screenshot: Option<Box<Screenshot>>,
}

/// The window manager's facts a host publishes before a frame; the `Default` is a host that
/// publishes none, a window nobody is closing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowFacts {
    pub(crate) close_requested: bool,
    pub(crate) placement: WindowPlacement,
    /// The content size in physical px the window returns to from maximized; `None` where the
    /// host tracks none, which reads as the live size.
    pub(crate) restored_size: Option<UVec2>,
    /// The window showed no frame since its last one, hidden by occlusion or a minimize.
    pub(crate) shown_again: bool,
}
