//! The offscreen host every device-needing audit draws through, so all callers share the target's format, usage and clear color.

use glam::UVec2;
use palantir::internals::{HeadlessTestGpuLease, OwnedTarget};
use palantir::{FrameReport, OffscreenHost, OffscreenTime, RgbaF32, Ui};

/// One offscreen host and the texture it draws into.
#[derive(Debug)]
pub(crate) struct OffscreenTarget {
    host: OffscreenHost,
    target: OwnedTarget,
}

impl OffscreenTarget {
    /// Not a `Harness`: the default offscreen path copies its backbuffer onto the target every frame, which keeps a submission in each still frame for the GPU gates to measure. The clock stands still.
    pub(crate) fn new(gpu: &HeadlessTestGpuLease, label: &str, surface: UVec2) -> Self {
        let mut host = OffscreenHost::builder(gpu.handles())
            .time(OffscreenTime::Manual)
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::TRANSPARENT;
        let target = gpu.target(label, surface);
        Self { host, target }
    }

    /// One frame; a caller drains the device after it, outside its count, so the next frame
    /// starts on an idle queue.
    pub(crate) fn frame(&mut self, dpr: f32, mut record: impl FnMut(&mut Ui)) -> FrameReport {
        self.host.frame(self.target.as_target(), dpr, &mut record)
    }
}
