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

use pollster::FutureExt;
use std::sync::OnceLock;

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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .block_on()
        .expect("request adapter (headless)");

    let timing_features = match timing {
        Timing::Instrumented => {
            adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY
                    | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                    | wgpu::Features::PIPELINE_STATISTICS_QUERY)
        }
        Timing::Bare => wgpu::Features::empty(),
    };

    // Match the production host: text `Params` rides immediates (push
    // constants), so the feature and a 16-byte budget are required.
    let mut limits = wgpu::Limits::default();
    limits.max_immediate_size = limits.max_immediate_size.max(16);

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(match timing {
                Timing::Instrumented => "palantir.bench.device.instrumented",
                Timing::Bare => "palantir.bench.device.bare",
            }),
            required_features: timing_features | wgpu::Features::IMMEDIATES,
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .block_on()
        .expect("request device");

    BenchGpu {
        device,
        queue,
        info: adapter.get_info(),
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
