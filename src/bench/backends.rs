//! The backends a case runs on: the CPU always, the GPU when the host has an adapter.

use crate::bench::case_group::BetweenCases;
use crate::gpu::bench_gpu::{BenchAdapter, BenchGpu, Timing};
use std::cell::OnceCell;
use std::env;

/// The backends a case may run on: the CPU always, the GPU when the host has an adapter, and the
/// devices the GPU cases run on.
///
/// The run owns the devices ([`crate::bench::run`]), so they drop when it ends, before the process
/// exits: a device a `static` held was never dropped (RFC 1440) and met `exit()` alive, where the
/// driver's teardown raced its own shader-compiler thread.
#[derive(Debug)]
pub struct Backends {
    /// The adapter the GPU cases open their device on; `None` on a host with none.
    adapter: Option<BenchAdapter>,
    /// A device per [`Timing`], opened by the first case that asks for it.
    devices: [OnceCell<BenchGpu>; 3],
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
            return Self::of(None);
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
        Self::of(adapter)
    }

    const fn of(adapter: Option<BenchAdapter>) -> Self {
        Self {
            adapter,
            devices: [const { OnceCell::new() }; 3],
        }
    }

    /// The device of `timing`, opened by the first case that asks for it.
    ///
    /// # Panics
    ///
    /// Panics when called between the cases of [`crate::bench::run`]: a group's body runs in
    /// every run, so a device it opens opens in a filtered run too, against the rule that a case
    /// builds its inputs in its own closure.
    #[track_caller]
    pub fn device(&self, timing: Timing) -> &BenchGpu {
        assert!(
            !BetweenCases::now(),
            "a bench opens its device in a case closure, not in its group's body"
        );
        let slot = match timing {
            Timing::Instrumented => 0,
            Timing::PassOnly => 1,
            Timing::Bare => 2,
        };
        self.devices[slot].get_or_init(|| BenchGpu::open(timing))
    }

    /// The backends a case with both runs on, the CPU first.
    pub fn all(&self) -> Vec<Backend> {
        if self.adapter.is_some() {
            vec![Backend::Cpu, Backend::Gpu]
        } else {
            vec![Backend::Cpu]
        }
    }

    /// The backends a GPU-only case runs on: the GPU, when there is one.
    pub(crate) fn gpu(&self) -> Option<Backend> {
        self.adapter.map(|_| Backend::Gpu)
    }

    /// The backend a case reading the frame's own backdrop timings runs on: the GPU, when its
    /// adapter writes timestamps between passes, where the frame writes them.
    pub(crate) fn encoder_timed_gpu(&self) -> Option<Backend> {
        self.adapter
            .filter(|adapter| adapter.encoder_timestamps)
            .map(|_| Backend::Gpu)
    }

    /// The backend a case timing one batch inside the pass runs on: the GPU, when its adapter
    /// writes in-pass timestamps.
    pub(crate) fn batch_timed_gpu(&self) -> Option<Backend> {
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
