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
//! the same shape `benches/alloc_*.rs` already had.
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
//! criterion's public setters. `Gate` draws that line — and the subtlety
//! is that test mode is signalled by an *absence*.
//!
//! What the runner decides, a driver is handed in a [`Run`] rather than
//! left to re-derive: two readings of the same argv are two things that
//! can disagree.

mod cli;
mod driver;

use cli::{Cli, Gate};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// `--note`: the caption a driver that appends to a results file
    /// records with the row.
    pub(crate) note: Option<&'a str>,
}

/// The bench target's entry point.
pub fn run() {
    // Opt-in drivers stay out: test mode is a smoke check that every
    // benchmark still executes, and the frame matrix is ~90 s of that —
    // unoptimized, since `cargo test` builds the dev profile.
    if Gate::parse_args().delegates() {
        let run = Run {
            arms: Arms::Both,
            recording: false,
            note: None,
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

    let want = cli.arms();
    for driver in DRIVERS.iter().filter(|d| cli.selects(d)) {
        let Some(arms) = driver.arms.overlap(want) else {
            continue;
        };
        let mut criterion = cli.configure((driver.config)());
        (driver.run)(
            &mut criterion,
            Run {
                arms,
                recording: cli.records(),
                note: cli.note.as_deref(),
            },
        );
    }
    cli.configure(Criterion::default()).final_summary();
}

/// The dhat allocation drivers. Not in the criterion registry: they take
/// no `Criterion`, measure allocations rather than time, and need a
/// `#[global_allocator]` that must not be linked into a timing binary —
/// so each keeps its own target.
pub use crate::host::bench::{alloc_free, alloc_free_gpu, alloc_resize};

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
