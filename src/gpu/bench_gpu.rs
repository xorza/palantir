//! The headless adapter and device every GPU bench driver runs on: one per
//! [`Timing`] flavour, process-static. Separate from [`crate::gpu::test_gpu`],
//! whose interprocess lock a bench must not block behind.

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::device::requested_gpu::RequestedGpu;
use crate::gpu::surface::render_target::{self, RenderTarget};
use crate::gpu::test_gpu::HeadlessTestGpuLease;
use crate::host::offscreen::{OffscreenHost, OffscreenHostBuilder};
use glam::UVec2;
use std::sync::OnceLock;

/// The render target format every bench uses, sRGB like the swapchain.
pub(crate) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Whether the device has the timestamp and pipeline-statistics features `GpuTimings` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Timing {
    /// Requests whichever timestamp and pipeline-statistics features the adapter has.
    Instrumented,
    /// [`Self::Instrumented`] without in-pass timestamps: a tiler splits the pass at each (the Pi 5's V3D reloads the target).
    PassOnly,
    /// Requests none, for a driver timing a pass the queries would write into.
    Bare,
}

/// A headless device and the adapter facts drivers report.
#[derive(Debug)]
pub(crate) struct BenchGpu {
    pub(crate) gpu: Gpu,
    pub(crate) info: wgpu::AdapterInfo,
    /// What was granted: empty under [`Timing::Bare`], else a subset of what the flavour asks for.
    pub(crate) timing_features: wgpu::Features,
}

fn build(timing: Timing) -> BenchGpu {
    let timing_features = match timing {
        Timing::Instrumented => DeviceRequirements::GPU_TIMING_FEATURES,
        Timing::PassOnly => DeviceRequirements::GPU_TIMING_FEATURES
            .difference(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        Timing::Bare => wgpu::Features::empty(),
    };
    let gpu = RequestedGpu::headless(PowerPreference::HighPerformance, timing_features)
        .expect("lease headless bench gpu");
    let timing_features = gpu.gpu.device.features() & timing_features;
    let info = gpu.adapter.get_info();
    BenchGpu {
        gpu: gpu.gpu,
        info,
        timing_features,
    }
}

impl BenchGpu {
    pub(crate) fn shared(timing: Timing) -> &'static BenchGpu {
        static INSTRUMENTED: OnceLock<BenchGpu> = OnceLock::new();
        static PASS_ONLY: OnceLock<BenchGpu> = OnceLock::new();
        static BARE: OnceLock<BenchGpu> = OnceLock::new();
        match timing {
            Timing::Instrumented => INSTRUMENTED.get_or_init(|| build(Timing::Instrumented)),
            Timing::PassOnly => PASS_ONLY.get_or_init(|| build(Timing::PassOnly)),
            Timing::Bare => BARE.get_or_init(|| build(Timing::Bare)),
        }
    }

    pub(crate) fn target(&self, label: &str, size: UVec2) -> BenchTarget {
        BenchTarget(render_target::internals::texture(
            &self.gpu.device,
            label,
            size,
            TARGET_FORMAT,
            HeadlessTestGpuLease::TARGET_USAGES,
        ))
    }

    /// Drains completed submissions without blocking; timing readback publishes on a poll.
    pub(crate) fn poll(&self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
    }

    /// Blocks until all submissions drain, keeping GPU work inside the measured window.
    pub(crate) fn wait(&self) {
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll");
    }

    /// An offscreen host builder on this device.
    pub(crate) fn offscreen_builder(&self) -> OffscreenHostBuilder {
        OffscreenHost::builder(self.gpu.clone())
    }

    /// The instrumentation granted; a missing bit silently empties a column.
    pub(crate) fn timing_summary(&self) -> String {
        format!(
            "TIMESTAMP_QUERY={} INSIDE_PASSES={} INSIDE_ENCODERS={} PIPELINE_STATS={}",
            self.timing_features
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            self.timing_features
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            self.timing_features
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            self.timing_features
                .contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
        )
    }
}

/// A texture a bench driver renders into.
#[derive(Debug)]
pub(crate) struct BenchTarget(wgpu::Texture);

impl BenchTarget {
    pub(crate) fn as_target(&self) -> RenderTarget<'_> {
        RenderTarget::new(&self.0)
    }

    pub(crate) fn view(&self) -> wgpu::TextureView {
        self.0.create_view(&wgpu::TextureViewDescriptor::default())
    }
}
