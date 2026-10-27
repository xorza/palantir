//! [`PresentSchedule`]: when a window's next frame runs, from how its last acquire went.

use crate::ui::frame_report::FrameReport;
use glam::UVec2;
use std::time::Duration;

/// First delay after a `Validation` acquire: about one frame at 60 Hz.
const ACQUIRE_RETRY_MIN: Duration = Duration::from_millis(16);

/// Ceiling for the back-off: two wake-ups a second.
const ACQUIRE_RETRY_MAX: Duration = Duration::from_millis(500);

/// What a frame's acquire gave, as the schedule reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AcquireOutcome {
    /// The frame painted nothing, so it acquired nothing.
    Skipped,
    Presented,
    /// Presented from a stale swapchain of this size.
    Suboptimal(UVec2),
    /// The swapchain is gone.
    Outdated,
    Timeout,
    /// The call was rejected.
    Validation,
    /// The window is not visible.
    Occluded,
}

/// When the next frame runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wake {
    Now,
    /// This long from now.
    After(Duration),
    /// At this frame time.
    At(Duration),
    Idle,
}

/// What a window does after a frame's acquire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PresentDecision {
    /// Rebuild the swapchain before the next acquire.
    pub(super) rebuild: bool,
    pub(super) wake: Wake,
}

/// The acquire history a window's next frames depend on.
#[derive(Debug, Default)]
pub(super) struct PresentSchedule {
    /// How long the next frame waits before retrying an acquire that failed validation; `None`
    /// while healthy. Other failures are transient and an immediate repaint settles them, but a
    /// validation failure repeats, so without a delay the host builds a full CPU draw list per
    /// loop iteration for nothing. Doubles to [`ACQUIRE_RETRY_MAX`].
    acquire_retry: Option<Duration>,
    /// The surface size a suboptimal acquire last rebuilt for: the swapchain rebuilds once per
    /// size, not every frame while a resize is pending.
    suboptimal_handled: Option<UVec2>,
}

impl PresentSchedule {
    /// Settle the frame `report` describes, whose acquire gave `outcome`.
    pub(super) fn after(
        &mut self,
        outcome: AcquireOutcome,
        report: &FrameReport,
    ) -> PresentDecision {
        let retry = self.acquire_retry.take();
        let mut rebuild = false;
        let repaint = match outcome {
            AcquireOutcome::Skipped | AcquireOutcome::Presented => report.repaint_requested,
            AcquireOutcome::Suboptimal(size) => {
                if self.suboptimal_handled != Some(size) {
                    self.suboptimal_handled = Some(size);
                    rebuild = true;
                }
                report.repaint_requested
            }
            AcquireOutcome::Outdated => {
                rebuild = true;
                true
            }
            AcquireOutcome::Timeout => true,
            AcquireOutcome::Validation => {
                self.acquire_retry = Some(retry.map_or(ACQUIRE_RETRY_MIN, |delay| {
                    (delay * 2).min(ACQUIRE_RETRY_MAX)
                }));
                true
            }
            AcquireOutcome::Occluded => false,
        };
        // Ahead of `repaint`, which every failing acquire asks for: the delay paces it.
        let wake = if let Some(delay) = self.acquire_retry {
            Wake::After(delay)
        } else if repaint {
            Wake::Now
        } else if let Some(due) = report.repaint_after {
            Wake::At(due)
        } else {
            Wake::Idle
        };
        PresentDecision { rebuild, wake }
    }
}
