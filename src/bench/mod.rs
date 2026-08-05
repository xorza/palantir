//! The benchmark runner, and the drivers it runs.
//!
//! Every driver lives in a `bench.rs` beside the code it measures — they
//! reach crate privates the supported surface will never carry, which is
//! why they cannot live under `benches/` themselves. That is also why
//! *this* is in the library rather than in the bench target: a
//! `benches/*.rs` is a separate crate and cannot name a `pub(crate)` fn,
//! so the registry has to be built here. The runner follows it in, which
//! keeps [`run`] and its selection rules unit-testable — a
//! `harness = false` target collects no `#[test]` fns at all.
//!
//! `benches/criterion.rs` is therefore a three-line call into [`run`],
//! and `benches/alloc.rs` the same into [`alloc::run`].
//!
//! ## Why this owns `main`
//!
//! Criterion's filter gates the `bench_function` call, not the setup a
//! driver runs before it — and that setup cannot move inside the
//! closure, which criterion invokes once per sample. So under
//! `criterion_main!` every driver paid every other driver's setup:
//! filtering to `damage` still cost ~11 s and two wgpu adapter requests,
//! all of it landing in a `perf` profile of the one benchmark asked for.
//! Selecting on the registry *before* calling a driver fixes that
//! exactly, and `criterion_main!` is ten lines of public API.
//!
//! ## Delegate-or-own
//!
//! `Criterion::configure_from_args` hard-exits on flags it doesn't know,
//! so it cannot be called on an argv carrying ours. And `criterion::Mode`
//! is `pub(crate)`: only profile mode is publicly reachable, **not**
//! `Test` or `List`. `Mode::Test` is what makes `cargo test --benches`
//! run each benchmark once instead of measuring it — and that command
//! does run this binary.
//!
//! So when cargo drives us in test or list mode, argv goes to
//! `configure_from_args` untouched. Otherwise `Cli` parses it and drives
//! criterion's public setters. `cli::delegates` draws that line — and the subtlety
//! is that test mode is signalled by an *absence*.
//!
//! What the runner decides, a driver is handed in a `Run` rather than
//! left to re-derive: two readings of the same argv are two things that
//! can disagree. Nothing here reads the environment — every input is a
//! declared flag, so `--help` is the whole surface.
//!
//! `cargo-criterion` integration rides `Criterion::default()`'s
//! `connection`, which the own-branch keeps by construction. Baselines
//! through `cargo criterion` are untested here.

pub mod alloc;
mod cli;
mod driver;

use cli::Cli;
use criterion::Criterion;
use driver::DRIVERS;

/// Which half of the pipeline is in play — on a driver row, what it
/// measures; on the command line, what the run wants. One vocabulary for
/// both sides, so selection is an intersection rather than two unrelated
/// switches.
///
/// Most drivers have a single arm, and for them this only decides
/// *whether* they run. A driver's `run` receives the resolved overlap
/// anyway, because the frame bench genuinely has both and has to know
/// which half was asked for.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arms {
    /// Touches no GPU.
    Cpu,
    /// Requests a wgpu adapter.
    Gpu,
    Both,
}

impl Arms {
    pub(crate) fn includes_cpu(self) -> bool {
        matches!(self, Arms::Cpu | Arms::Both)
    }

    pub(crate) fn includes_gpu(self) -> bool {
        matches!(self, Arms::Gpu | Arms::Both)
    }

    /// What a driver offering `self` should run when the caller asked
    /// for `want`, or `None` when they share nothing — the whole
    /// selection rule.
    pub(crate) fn overlap(self, want: Arms) -> Option<Arms> {
        match (
            self.includes_cpu() && want.includes_cpu(),
            self.includes_gpu() && want.includes_gpu(),
        ) {
            (true, true) => Some(Arms::Both),
            (true, false) => Some(Arms::Cpu),
            (false, true) => Some(Arms::Gpu),
            (false, false) => None,
        }
    }
}

/// What the runner resolved for one driver's invocation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Run<'a> {
    /// The half of the pipeline to exercise — [`Arms::overlap`] of what
    /// the driver offers against what the command line asked for.
    /// Single-arm drivers ignore it.
    pub(crate) arms: Arms,
    /// Whether criterion will write `estimates.json` for this run.
    ///
    /// False in test mode and under `--profile-time`, both of which
    /// report "Analysis Disabled" and record nothing. A driver that
    /// reads its own numbers back afterwards has to skip when this is
    /// false, or it files a row of "estimates not found".
    pub(crate) recording: bool,
    /// Every knob the command line carries for a driver that renders the
    /// shared fixture and files a results row. The frame bench is the
    /// only one today; a second would read the same struct rather than
    /// grow `Run` sideways.
    pub(crate) fixture: Fixture<'a>,
}

/// The surface the shared fixture renders into, and how the row it files
/// is captioned. Defaults live with the bench that uses them — `None`
/// means "whatever that bench considers normal", so the runner never has
/// to know a widget's dimensions to hand one over.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fixture<'a> {
    /// `--size <W>x<H>`: the physical surface every arm renders into.
    pub(crate) size: Option<glam::UVec2>,
    /// `--scale`: device pixel ratio.
    pub(crate) scale: Option<f32>,
    /// `--machine`: which per-machine results file the row lands in.
    /// Defaults to the short hostname.
    pub(crate) machine: Option<&'a str>,
    /// `--note`: the row's why-was-this-measured caption.
    pub(crate) note: Option<&'a str>,
}

/// The bench target's entry point.
pub fn run() {
    // Opt-in drivers stay out: test mode is a smoke check that every
    // benchmark still executes, and the frame matrix is ~90 s of that —
    // unoptimized, since `cargo test` builds the dev profile.
    let argv: Vec<String> = std::env::args().collect();
    if cli::delegates(argv.iter().map(String::as_str)) {
        let run = Run {
            arms: Arms::Both,
            recording: false,
            fixture: Fixture::default(),
        };
        for driver in DRIVERS.iter().filter(|d| !d.opt_in) {
            let mut criterion = (driver.config)().configure_from_args();
            (driver.run)(&mut criterion, run);
        }
        Criterion::default().configure_from_args().final_summary();
        return;
    }

    let cli = Cli::parse_args();

    if cli.list_drivers {
        for driver in DRIVERS {
            let opt_in = if driver.opt_in { "  (opt-in)" } else { "" };
            println!("{:<16} {:?}{opt_in}", driver.name, driver.arms);
        }
        return;
    }

    cli.validate(DRIVERS);

    for driver in DRIVERS.iter().filter(|d| cli.selects(d)) {
        let Some(arms) = driver.arms.overlap(cli.arms) else {
            continue;
        };
        let mut criterion = cli.configure((driver.config)());
        (driver.run)(
            &mut criterion,
            Run {
                arms,
                recording: cli.records(),
                fixture: cli.fixture(),
            },
        );
    }
    cli.configure(Criterion::default()).final_summary();
}

#[cfg(test)]
mod tests {
    use crate::bench::Arms;

    /// `overlap` is the entire selection rule, so pin its table. `None`
    /// is the only answer that skips a driver.
    #[test]
    fn overlap_selects_the_shared_half() {
        use Arms::{Both, Cpu, Gpu};
        let cases = [
            // (driver arms, requested, resolved)
            (Cpu, Cpu, Some(Cpu)),
            (Cpu, Gpu, None),
            (Cpu, Both, Some(Cpu)),
            (Gpu, Cpu, None),
            (Gpu, Gpu, Some(Gpu)),
            (Gpu, Both, Some(Gpu)),
            (Both, Cpu, Some(Cpu)),
            (Both, Gpu, Some(Gpu)),
            (Both, Both, Some(Both)),
        ];
        for (have, want, expect) in cases {
            assert_eq!(have.overlap(want), expect, "{have:?} ∩ {want:?}");
        }
    }
}
