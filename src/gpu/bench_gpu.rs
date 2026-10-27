//! The headless adapter and device every GPU bench driver runs on: one per
//! [`Timing`] flavor, process-static. Separate from [`crate::gpu::test_gpu`],
//! whose interprocess lock a bench must not block behind.

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::device::requested_gpu::GpuRequest;
use crate::gpu::device::requested_gpu::RequestedGpu;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use glam::UVec2;
use std::sync::OnceLock;

/// The render target format every bench uses, sRGB like the swapchain.
pub(crate) const TARGET_FORMAT: TargetFormat = TargetFormat::RGBA8_SRGB;

/// The adapter policy every bench device opens with, and [`BenchGpu::probe`] picks by.
const BENCH_POWER: PowerPreference = PowerPreference::HighPerformance;

/// What the adapter the bench devices open on offers, read without opening a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BenchAdapter {
    /// Whether it writes timestamps inside a render pass, which a batch's own GPU time needs.
    pub(crate) pass_timestamps: bool,
}

/// Whether the device has the timestamp and pipeline-statistics features `GpuTimings` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    /// Requests whichever timestamp and pipeline-statistics features the adapter has.
    Instrumented,
    /// [`Self::Instrumented`] without in-pass timestamps: a tiler splits the pass at each (the Pi 5's V3D reloads the target).
    PassOnly,
    /// Requests none, for a driver timing a pass the queries would write into.
    Bare,
}

/// A headless device and the adapter facts drivers report.
#[derive(Debug)]
pub struct BenchGpu {
    pub(crate) gpu: Gpu,
    pub(crate) info: wgpu::AdapterInfo,
    /// What was granted: empty under [`Timing::Bare`], else a subset of what the flavor asks for.
    pub(crate) timing_features: wgpu::Features,
}

/// The device of `timing`, its adapter and instrumentation said once on stderr: a missing
/// feature silently empties a timing.
#[expect(
    clippy::print_stderr,
    reason = "a bench reports the device it measures on, which criterion does not"
)]
fn build(timing: Timing) -> BenchGpu {
    let timing_features = match timing {
        Timing::Instrumented => DeviceRequirements::GPU_TIMING_FEATURES,
        Timing::PassOnly => DeviceRequirements::GPU_TIMING_FEATURES
            .difference(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        Timing::Bare => wgpu::Features::empty(),
    };
    let gpu =
        RequestedGpu::headless(BENCH_POWER, timing_features).expect("lease headless bench gpu");
    let timing_features = gpu.gpu.device.features() & timing_features;
    let info = gpu.adapter.get_info();
    let gpu = BenchGpu {
        gpu: gpu.gpu,
        info,
        timing_features,
    };
    eprintln!("palantir benches: {timing:?} device, {}", gpu.summary());
    gpu
}

impl BenchGpu {
    /// The adapter the bench devices open on, picked as [`Self::shared`] picks it but with no
    /// device opened; `None` when the host has none.
    pub(crate) fn probe() -> Option<BenchAdapter> {
        let instance = GpuRequest::instance().ok()?;
        let request = GpuRequest {
            label: "palantir.bench.probe",
            power_preference: BENCH_POWER,
            optional: wgpu::Features::empty(),
            compatible_surface: None,
        };
        let features = request.adapter(&instance).ok()?.features();
        Some(BenchAdapter {
            pass_timestamps: features.contains(
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
            ),
        })
    }

    /// The process's device of `timing`, opened by the first caller.
    pub fn shared(timing: Timing) -> &'static Self {
        static INSTRUMENTED: OnceLock<BenchGpu> = OnceLock::new();
        static PASS_ONLY: OnceLock<BenchGpu> = OnceLock::new();
        static BARE: OnceLock<BenchGpu> = OnceLock::new();
        match timing {
            Timing::Instrumented => INSTRUMENTED.get_or_init(|| build(Timing::Instrumented)),
            Timing::PassOnly => PASS_ONLY.get_or_init(|| build(Timing::PassOnly)),
            Timing::Bare => BARE.get_or_init(|| build(Timing::Bare)),
        }
    }

    /// A `size` render target named `label`, in the swapchain's format.
    pub fn target(&self, label: &str, size: UVec2) -> OwnedTarget {
        OwnedTarget::window(&self.gpu, label, size, TARGET_FORMAT)
    }

    /// Drains completed submissions without blocking; timing readback publishes on a poll.
    pub(crate) fn poll(&self) {
        self.gpu.device.poll(wgpu::PollType::Poll).ok();
    }

    /// Blocks until all submissions drain, keeping GPU work inside the measured window.
    pub fn wait(&self) {
        self.gpu.wait();
    }

    /// The device's handles, for a host on it.
    pub fn handles(&self) -> Gpu {
        self.gpu.clone()
    }

    /// The adapter and the instrumentation granted.
    fn summary(&self) -> String {
        format!(
            "adapter={} backend={:?} TIMESTAMP_QUERY={} INSIDE_PASSES={} INSIDE_ENCODERS={} \
             PIPELINE_STATS={}",
            self.info.name,
            self.info.backend,
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
