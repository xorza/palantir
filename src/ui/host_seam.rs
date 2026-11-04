//! [`HostSeam`]: what a `Ui` and its host hand each other across frames.

use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::window::waker::Waker;
use crate::window::window_frame_state::WindowFrameState;
use crate::window::window_requests::WindowRequests;

/// What a `Ui` and the host of its window hand each other: the requests the recorder leaves for
/// the host, the facts the host sets before a frame, the GPU timings the backend publishes, and
/// the waker the host gives the window. One owner, so the seam between the two reads as one.
#[derive(Debug)]
pub(super) struct HostSeam {
    /// Recorder-to-host requests retained across frames.
    pub(super) window_requests: WindowRequests,
    /// Host-to-recorder facts refreshed before each windowed frame.
    pub(super) window_frame: WindowFrameState,
    /// This window's latest GPU timing sample, which the backend publishes into on each of the
    /// window's submits: a window reads its own frames, not the last window's.
    pub(super) gpu_pass_stats: GpuPassStats,
    /// Asks this window's host for a frame from any thread; the host sets it once the window
    /// opens.
    pub(super) waker: Waker,
}

impl Default for HostSeam {
    fn default() -> Self {
        Self {
            window_requests: WindowRequests::default(),
            window_frame: WindowFrameState::default(),
            gpu_pass_stats: GpuPassStats::default(),
            waker: Waker::new(|| {}),
        }
    }
}
