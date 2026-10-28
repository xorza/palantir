//! The backends a case runs on: the CPU always, the GPU when the host has an adapter.

use crate::gpu::bench_gpu::{BenchAdapter, BenchGpu};
use std::env;

/// The backends a case may run on: the CPU always, the GPU when the host has an adapter.
#[derive(Clone, Copy, Debug)]
pub struct Backends {
    /// The adapter the GPU cases open their device on; `None` on a host with none.
    adapter: Option<BenchAdapter>,
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
    /// The environment variable that, set to anything but empty, keeps a run off the GPU.
    pub const NO_GPU: &str = "PALANTIR_BENCH_NO_GPU";

    /// The backends of this host, from the adapter the GPU cases would open: no device is opened,
    /// so a run that filters to CPU cases opens none. Each kind of GPU case the host cannot run is
    /// said once on stderr, as those cases are then missing from the run.
    ///
    /// With [`NO_GPU`](Self::NO_GPU) set, nothing is probed and no GPU case registers. A probe
    /// creates a wgpu instance, which loads the GPU driver into the process; a profile of CPU cases
    /// that sets it holds none of the driver's work.
    #[expect(
        clippy::print_stderr,
        reason = "the notice is what keeps missing GPU cases from reading as a full run"
    )]
    pub fn probe() -> Self {
        if env::var_os(Self::NO_GPU).is_some_and(|value| !value.is_empty()) {
            eprintln!("palantir benches: {} is set, so no GPU cases", Self::NO_GPU);
            return Self { adapter: None };
        }
        let adapter = BenchGpu::probe();
        match adapter {
            None => eprintln!("palantir benches: no GPU adapter, so no GPU cases"),
            Some(adapter) => {
                if !adapter.pass_timestamps {
                    eprintln!(
                        "palantir benches: the adapter writes no in-pass timestamps, so no \
                         batch-timed GPU cases"
                    );
                }
                if !adapter.encoder_timestamps {
                    eprintln!(
                        "palantir benches: the adapter writes no timestamps between passes, so \
                         no cases reading the frame's backdrop timings, and runs of passes read \
                         wall time, from submit to done"
                    );
                }
            }
        }
        Self { adapter }
    }

    /// The backends a case with both runs on, the CPU first.
    pub fn all(self) -> Vec<Backend> {
        if self.adapter.is_some() {
            vec![Backend::Cpu, Backend::Gpu]
        } else {
            vec![Backend::Cpu]
        }
    }

    /// The backends a GPU-only case runs on: the GPU, when there is one.
    pub(crate) fn gpu(self) -> Option<Backend> {
        self.adapter.map(|_| Backend::Gpu)
    }

    /// The backend a case reading the frame's own backdrop timings runs on: the GPU, when its
    /// adapter writes timestamps between passes, where the frame writes them.
    pub(crate) fn encoder_timed_gpu(self) -> Option<Backend> {
        self.adapter
            .filter(|adapter| adapter.encoder_timestamps)
            .map(|_| Backend::Gpu)
    }

    /// The backend a case timing one batch inside the pass runs on: the GPU, when its adapter
    /// writes in-pass timestamps.
    pub(crate) fn batch_timed_gpu(self) -> Option<Backend> {
        self.adapter
            .filter(|adapter| adapter.pass_timestamps)
            .map(|_| Backend::Gpu)
    }
}

impl Backend {
    /// The backend's part of a case's id.
    pub(crate) const fn id(self) -> &'static str {
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
