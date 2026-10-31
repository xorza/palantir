//! [`FrameSummary`]: what a window's last frame reported, readable from any thread.

use std::time::Duration;

/// A window's last frame, as [`HostHandle::last_frame`](crate::HostHandle::last_frame) reads it:
/// the plain-data part of its [`FrameReport`](crate::FrameReport).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSummary {
    /// The frame's [`Ui::frame_id`](crate::Ui::frame_id).
    pub frame_id: u64,
    /// How many events [`HostHandle::send_input`](crate::HostHandle::send_input) delivered to the
    /// window before this frame. A sender that counts what it sent knows a frame ran after its
    /// last event once this reaches the count, whatever frames were already due when it sent.
    /// The count starts at the window's open: a window opened again under the same token counts
    /// from zero.
    pub sent_inputs: u64,
    /// See [`FrameReport::repaint_requested`](crate::FrameReport::repaint_requested).
    pub repaint_requested: bool,
    /// See [`FrameReport::repaint_after`](crate::FrameReport::repaint_after).
    pub repaint_after: Option<Duration>,
    /// What [`App::is_busy`](crate::App::is_busy) said after the frame.
    pub busy: bool,
}
