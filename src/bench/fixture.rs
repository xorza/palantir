//! The policy every case follows: its kind and sampling, the backends it runs on, and the window
//! a GPU case draws into.

use crate::gpu::bench_gpu::BenchGpu;
use crate::host::headless_window::HeadlessWindow;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, SamplingMode};
use glam::UVec2;
use std::time::Duration;

/// What a case times, the first part of its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One stage, one widget or one operation on a made input.
    Atomic,
    /// Palantir's frame, or a chain of its stages from record onward, over a scene.
    Integration,
}

impl Kind {
    /// The group `name` of this kind, sampled as `scale` says.
    pub fn group<'a>(
        self,
        c: &'a mut Criterion,
        name: &str,
        scale: Scale,
    ) -> BenchmarkGroup<'a, WallTime> {
        let kind = match self {
            Self::Atomic => "atomic",
            Self::Integration => "integration",
        };
        let mut group = c.benchmark_group(format!("{kind}/{name}"));
        scale.apply(&mut group);
        group
    }
}

/// How long one iteration of a case takes, which sets how criterion samples it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Microseconds an iteration.
    Sample,
    /// Up to 300 ms an iteration (every case here, as measured), so fewer and longer samples.
    Frame,
}

impl Scale {
    /// Sets the sampling of the cases `group` registers from here on.
    #[expect(
        clippy::disallowed_methods,
        reason = "`Scale::apply` is the one place a group's sampling is set"
    )]
    pub fn apply(self, group: &mut BenchmarkGroup<'_, WallTime>) {
        match self {
            Self::Sample => group
                .sample_size(50)
                .sampling_mode(SamplingMode::Auto)
                .warm_up_time(Duration::from_millis(500))
                .measurement_time(Duration::from_secs(1)),
            Self::Frame => group
                .sample_size(10)
                .sampling_mode(SamplingMode::Auto)
                .warm_up_time(Duration::from_secs(1))
                .measurement_time(Duration::from_secs(3)),
        };
    }
}

/// The backends a case may run on: the CPU always, the GPU when the host has an adapter.
#[derive(Clone, Copy, Debug)]
pub struct Backends {
    has_adapter: bool,
}

/// One backend a case runs on, the first part of its case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// No device: the case runs on the CPU alone.
    Cpu,
    /// A device opens for the case.
    Gpu,
}

impl Backends {
    /// The backends of this host, by listing its adapters: no device is opened, so a run that
    /// filters to CPU cases opens none. A host with no adapter says so once on stderr, as its
    /// GPU cases are then missing from the run.
    #[expect(
        clippy::print_stderr,
        reason = "the notice is what keeps missing GPU cases from reading as a full run"
    )]
    pub fn probe() -> Self {
        let has_adapter = BenchGpu::adapter_exists();
        if !has_adapter {
            eprintln!("palantir benches: no GPU adapter, so no GPU cases");
        }
        Self { has_adapter }
    }

    /// The backends a case with both runs on, the CPU first.
    pub fn all(self) -> Vec<Backend> {
        if self.has_adapter {
            vec![Backend::Cpu, Backend::Gpu]
        } else {
            vec![Backend::Cpu]
        }
    }

    /// The backends a GPU-only case runs on: the GPU, when there is one.
    pub fn gpu(self) -> Option<Backend> {
        self.has_adapter.then_some(Backend::Gpu)
    }
}

impl Backend {
    /// The backend's part of a case's id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
        }
    }

    /// `rest` under this backend: `gpu/cached`.
    pub fn case(self, rest: &str) -> String {
        format!("{}/{rest}", self.id())
    }
}

/// A window of `size` at scale 1 on `gpu` whose panels paint no theme chrome, so a GPU case's own
/// draws are all a frame holds.
pub(crate) fn plain_window(gpu: &BenchGpu, size: UVec2, collect_gpu_stats: bool) -> HeadlessWindow {
    let mut window = HeadlessWindow::builder(gpu.handles())
        .collect_gpu_stats(collect_gpu_stats)
        .open(size, 1.0);
    window.ui().theme_mut().panel_chrome = None;
    window
}
