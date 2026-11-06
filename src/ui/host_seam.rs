//! [`HostSeam`]: what a `Ui` and its host hand each other across frames.

use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::display::Display;
use crate::primitives::geometry::size::Size;
use crate::window::screenshot::{Screenshot, ScreenshotId};
use crate::window::waker::Waker;
use crate::window::window_frame_state::WindowFrameState;
use crate::window::window_geometry::WindowGeometry;
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
    /// window's submits: a window reads its own frames, not the last window's. Boxed, as every
    /// pass walks `&mut Ui` and the sample is cold (`internals::hot_struct_sizes`).
    pub(super) gpu_pass_stats: Box<GpuPassStats>,
    /// Asks this window's host for a frame from any thread; the host sets it once the window
    /// opens.
    pub(super) waker: Waker,
}

impl HostSeam {
    /// Whether the window manager asked this window to close this frame.
    pub(super) const fn close_requested(&self) -> bool {
        self.window_frame.facts.close_requested
    }

    /// The image `id` asked for, once it landed.
    pub(super) fn screenshot_taken(&self, id: ScreenshotId) -> Option<&Screenshot> {
        self.window_frame
            .screenshot
            .as_deref()
            .filter(|screenshot| screenshot.id() == id)
    }

    /// The window's geometry at `display`, as [`Ui::window_geometry`](crate::Ui::window_geometry)
    /// says.
    pub(super) fn window_geometry(&self, display: Display) -> WindowGeometry {
        let physical = self
            .window_frame
            .facts
            .restored_size
            .unwrap_or(display.physical);
        // A window skips its frames while hidden, minimized included, so one that records has an
        // extent, as `WindowConfig::with_geometry` asks.
        debug_assert!(
            physical.min_element() > 0,
            "a recording window has a size: {physical}"
        );
        let scale = display.system_scale.get();
        WindowGeometry {
            inner_size: Size::new(physical.x as f32 / scale, physical.y as f32 / scale),
            placement: self.window_frame.facts.placement,
        }
    }
}

impl Default for HostSeam {
    fn default() -> Self {
        Self {
            window_requests: WindowRequests::default(),
            window_frame: WindowFrameState::default(),
            gpu_pass_stats: Box::default(),
            waker: Waker::new(|| {}),
        }
    }
}
