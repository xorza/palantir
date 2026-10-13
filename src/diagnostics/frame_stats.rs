//! The counters one frame publishes, which the frame-stats readout draws.

use std::time::Duration;

/// One frame's diagnostic counters, as `Ui::frame_stats` snapshots them; a snapshot because the readout records through `&mut Ui`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameStats {
    pub(crate) frame_id: u64,
    pub(crate) render_frame_id: u64,
    pub(crate) fps: f32,
    pub(crate) settle_frames: u32,
    /// Whole-pass GPU time of the last frame that read a timestamp back.
    pub(crate) gpu: Option<Duration>,
}
