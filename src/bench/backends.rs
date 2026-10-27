//! The backends a case runs on: the CPU always, the GPU when the host has an adapter.

use crate::gpu::bench_gpu::BenchGpu;

/// The backends a case may run on: the CPU always, the GPU when the host has an adapter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Backends {
    has_adapter: bool,
}

/// One backend a case runs on, the first part of its case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
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
    pub(crate) fn probe() -> Self {
        let has_adapter = BenchGpu::adapter_exists();
        if !has_adapter {
            eprintln!("palantir benches: no GPU adapter, so no GPU cases");
        }
        Self { has_adapter }
    }

    /// The backends a case with both runs on, the CPU first.
    pub(crate) fn all(self) -> Vec<Backend> {
        if self.has_adapter {
            vec![Backend::Cpu, Backend::Gpu]
        } else {
            vec![Backend::Cpu]
        }
    }

    /// The backends a GPU-only case runs on: the GPU, when there is one.
    pub(crate) fn gpu(self) -> Option<Backend> {
        self.has_adapter.then_some(Backend::Gpu)
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
    pub(crate) fn case(self, rest: &str) -> String {
        format!("{}/{rest}", self.id())
    }
}
