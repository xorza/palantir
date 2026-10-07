//! The offscreen host every device-needing audit draws through, so all callers share the target's format, usage and clear colour.

#![expect(
    clippy::disallowed_types,
    reason = "an outside consumer of the published surface, where naming a wgpu type is the point"
)]

use std::time::Duration;

use glam::UVec2;
use palantir::internals::HeadlessTestGpuLease;
use palantir::internals::record_app::RecordApp;
use palantir::{FixedClock, FrameReport, OffscreenHost, RenderTarget, RgbaF32, Ui};

/// One offscreen host and the texture it draws into.
#[derive(Debug)]
pub(crate) struct OffscreenTarget {
    host: OffscreenHost,
    texture: wgpu::Texture,
}

impl OffscreenTarget {
    /// The public offscreen path always copies from its backbuffer, so callers exclude the direct-present path. The clock stands still.
    pub(crate) fn new(gpu: &HeadlessTestGpuLease, label: &str, surface: UVec2) -> Self {
        let mut host = OffscreenHost::builder(gpu.handles())
            .clock(FixedClock::new(Duration::ZERO))
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::TRANSPARENT;
        let texture = gpu.target(label, surface);
        Self { host, texture }
    }

    /// One frame, drained before it returns so GPU execution lands in the frame that submitted it.
    pub(crate) fn frame(
        &mut self,
        gpu: &HeadlessTestGpuLease,
        dpr: f32,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        let report = self.host.frame(
            RenderTarget::new(&self.texture),
            dpr,
            &mut RecordApp::new(record),
        );
        gpu.wait();
        report
    }
}
