//! [`FrameSummary`]: what a window's last frame reported, readable from any thread.

use std::time::Duration;

/// A window's last frame, as `HostHandle::last_frame` reads it: the plain-data part of its
/// [`FrameReport`](crate::FrameReport), for a [`Remote`](crate::Remote)'s settle. Crate-private, as
/// its counts are the remote protocol's; an app asks [`HostHandle::is_open`](crate::HostHandle::is_open)
/// and [`HostHandle::is_hidden`](crate::HostHandle::is_hidden).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameSummary {
    /// The frame's [`Ui::record_frame_id`](crate::Ui::record_frame_id).
    pub(crate) record_frame_id: u64,
    /// How many events a [`Remote`](crate::Remote) sent through the handle reached the window
    /// before this frame, its inputs and its queries. A sender that counts what it sent knows a
    /// frame ran after its last event once this reaches the count, whatever frames were already
    /// due when it sent.
    /// The count starts at the window's open: a window opened again under the same token counts
    /// from zero.
    pub(crate) sent_events: u64,
    /// See [`FrameReport::repaint_requested`](crate::FrameReport::repaint_requested).
    pub(crate) repaint_requested: bool,
    /// See [`FrameReport::repaint_at`](crate::FrameReport::repaint_at).
    pub(crate) repaint_at: Option<Duration>,
    /// What [`App::is_busy`](crate::App::is_busy) said after the frame.
    pub(crate) busy: bool,
}
