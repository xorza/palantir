//! The headless adapter and device every GPU bench driver runs on.
//!
//! One per [`Timing`] flavour, process-static: the criterion target
//! holds every driver, so they run in one process and sharing a device
//! saves an adapter request per driver that asks for the same one.
//!
//! Separate from [`crate::gpu::test_gpu`], which serves the test
//! suites, because the two want opposite things. That one takes an
//! interprocess lock so parallel test binaries don't contend for the
//! adapter; a bench must not block for minutes behind someone else's
//! lock.

use crate::gpu::device_requirements::DeviceRequirements;
use crate::gpu::power_preference::PowerPreference;
use crate::gpu::render_target::{self, RenderTarget};
use crate::gpu::requested_gpu::Gpu;
use crate::gpu::requested_gpu::RequestedGpu;
use crate::host::offscreen::{OffscreenHost, OffscreenHostBuilder};
use glam::UVec2;
use std::sync::OnceLock;

/// Every bench target renders into this. sRGB so the encoded colour
/// matches what the swapchain would receive in production. Public to the
/// crate because a driver constructing a backend has to pass the same
/// one, and two constants that must agree are one waiting to drift.
pub(crate) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Whether the device carries the timestamp and pipeline-statistics
/// features the backend's `GpuTimings` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Timing {
    /// Intersect in whatever of `TIMESTAMP_QUERY`,
    /// `TIMESTAMP_QUERY_INSIDE_PASSES` and `PIPELINE_STATISTICS_QUERY`
    /// the adapter advertises, so instrumentation can publish
    /// whole-pass and per-batch durations. Missing bits degrade
    /// individually rather than failing the request.
    Instrumented,
    /// Ask for none of them, for a driver timing a pass the queries
    /// would write into. The host also passes `collect_gpu_stats(false)`,
    /// but not requesting the feature at all closes the door on a future
    /// default that writes timestamps into the very pass being measured.
    Bare,
}

/// A headless device and the adapter facts drivers report alongside
/// their numbers.
#[derive(Debug)]
pub(crate) struct BenchGpu {
    pub(crate) gpu: Gpu,
    pub(crate) info: wgpu::AdapterInfo,
    /// What was actually granted — empty under [`Timing::Bare`], and
    /// under [`Timing::Instrumented`] only what the adapter had.
    pub(crate) timing_features: wgpu::Features,
}

fn build(timing: Timing) -> BenchGpu {
    let timing_features = match timing {
        Timing::Instrumented => DeviceRequirements::GPU_TIMING_FEATURES,
        Timing::Bare => wgpu::Features::empty(),
    };
    // Palantir's own needs — the immediates feature and its 16-byte budget —
    // come from `RequestedGpu`, so the bench device cannot drift from the one
    // the production host builds.
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
    /// The process-static GPU for `timing`, built on first ask.
    pub(crate) fn shared(timing: Timing) -> &'static BenchGpu {
        static INSTRUMENTED: OnceLock<BenchGpu> = OnceLock::new();
        static BARE: OnceLock<BenchGpu> = OnceLock::new();
        match timing {
            Timing::Instrumented => INSTRUMENTED.get_or_init(|| build(Timing::Instrumented)),
            Timing::Bare => BARE.get_or_init(|| build(Timing::Bare)),
        }
    }

    /// A render target of `size`, with the usages every driver needs:
    /// draw into it, and copy either way for readback and clears.
    ///
    /// `label` shows up in RenderDoc and in wgpu's validation errors, so
    /// it should name the driver, not the shape.
    pub(crate) fn target(&self, size: UVec2, label: &str) -> BenchTarget {
        BenchTarget(self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }))
    }

    /// Drain one round of completed submissions without blocking. The
    /// timing readback publishes through a map callback, which fires on a
    /// poll, so a driver that reads a pass time needs one of these after
    /// the submit it wants the number for.
    pub(crate) fn poll(&self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
    }

    /// Block until every submission has drained. Between iterations this
    /// is what puts GPU execution inside the measured window instead of
    /// the next one's.
    pub(crate) fn wait(&self) {
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll");
    }

    /// An offscreen host on this device. Returns the builder rather than
    /// a host: drivers differ on GPU stats and theme, and those reading
    /// at the call site is the point.
    pub(crate) fn offscreen_builder(&self) -> OffscreenHostBuilder {
        OffscreenHost::builder(self.gpu.clone())
    }

    /// Which instrumentation the device ended up with, for a driver that
    /// prints it alongside its numbers — the bits vary by adapter, and a
    /// missing one silently empties a column.
    pub(crate) fn timing_summary(&self) -> String {
        format!(
            "TIMESTAMP_QUERY={} INSIDE_PASSES={} PIPELINE_STATS={}",
            self.timing_features
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            self.timing_features
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            self.timing_features
                .contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
        )
    }
}

/// A texture a bench driver renders into.
///
/// Owned rather than borrowed: a driver keeps its targets alive across every
/// iteration, and a resizing driver keeps a pool of them.
#[derive(Debug)]
pub(crate) struct BenchTarget(wgpu::Texture);

impl BenchTarget {
    pub(crate) fn as_target(&self) -> RenderTarget<'_> {
        RenderTarget::from(&self.0)
    }

    /// A colour-attachment view, for a driver that opens its own pass
    /// instead of going through a host.
    pub(crate) fn view(&self) -> wgpu::TextureView {
        self.0.create_view(&wgpu::TextureViewDescriptor::default())
    }
}
