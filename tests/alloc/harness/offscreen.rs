//! The offscreen host every audit that needs a device draws through:
//! the gates in `gates/on_gpu.rs`, which ask what the driver costs.
//!
//! Written once because every caller has to agree on the target's format,
//! usage and clear colour. Those decide how much submission work a frame
//! owes, and a caller that differed would be measuring against a floor
//! nobody else's number shares.

// Reaches Palantir the way an outside consumer does, through the published
// surface, where naming a wgpu type is the point. `clippy.toml` keeps them out
// of the library's own modules.
#![allow(clippy::disallowed_types)]

use std::time::Duration;

use glam::UVec2;
use palantir::internals::HeadlessTestGpuLease;
use palantir::internals::record_app::RecordApp;
use palantir::{FixedClock, FrameReport, OffscreenHost, RgbaF32, Ui};

/// One offscreen host and the texture it draws into.
#[derive(Debug)]
pub(crate) struct OffscreenTarget {
    host: OffscreenHost,
    texture: wgpu::Texture,
}

impl OffscreenTarget {
    /// The public offscreen path always copies from its backbuffer, so
    /// what every caller pins excludes the direct-present path. The clock
    /// stands still, so no frame's work depends on how fast the last one
    /// ran.
    pub(crate) fn new(gpu: &HeadlessTestGpuLease, label: &str, surface: UVec2) -> Self {
        let mut host = OffscreenHost::builder(gpu.handles())
            .clock(FixedClock::new(Duration::ZERO))
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::TRANSPARENT;
        let texture = gpu.target(label, surface);
        Self { host, texture }
    }

    /// One frame, drained before it returns.
    ///
    /// Draining here is what puts GPU execution inside the frame that
    /// submitted it instead of the next one's window.
    pub(crate) fn frame(
        &mut self,
        gpu: &HeadlessTestGpuLease,
        dpr: f32,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        let report = self
            .host
            .frame(&self.texture, dpr, &mut RecordApp::new(record));
        gpu.wait();
        report
    }
}
