//! The headless adapter and device every GPU bench driver runs on.
//!
//! One per [`Timing`] flavour, process-static: the criterion target
//! holds every driver, so they run in one process and sharing a device
//! saves an adapter request per driver that asks for the same one.
//!
//! Separate from [`crate::host::test_gpu`], which serves the test
//! suites, because the two want opposite things. That one takes
//! `LowPower` and an interprocess lock so parallel test binaries don't
//! contend; a bench wants the discrete GPU and must not block for
//! minutes behind someone else's lock.

use crate::host::headless_gpu::HeadlessGpu;
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
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) info: wgpu::AdapterInfo,
    /// What was actually granted — empty under [`Timing::Bare`], and
    /// under [`Timing::Instrumented`] only what the adapter had.
    pub(crate) timing_features: wgpu::Features,
}

fn build(timing: Timing) -> BenchGpu {
    let timing_features = match timing {
        Timing::Instrumented => {
            wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY
        }
        Timing::Bare => wgpu::Features::empty(),
    };
    // Palantir's own needs — the immediates feature and its 16-byte budget —
    // come from `HeadlessGpu`, so the bench device cannot drift from the one
    // the production host builds.
    let gpu = HeadlessGpu::new(wgpu::PowerPreference::HighPerformance, timing_features)
        .expect("lease headless bench gpu");
    let timing_features = gpu.device.features() & timing_features;
    let info = gpu.adapter.get_info();
    BenchGpu {
        device: gpu.device,
        queue: gpu.queue,
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
    pub(crate) fn target(&self, size: UVec2, label: &str) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Block until every submission has drained. Between iterations this
    /// is what puts GPU execution inside the measured window instead of
    /// the next one's.
    pub(crate) fn wait(&self) {
        self.device
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
        OffscreenHost::builder(self.device.clone(), self.queue.clone())
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
