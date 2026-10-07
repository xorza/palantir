//! The benchmark runner, and the drivers it runs.
//!
//! Each driver lives in a `bench.rs` beside the code it measures, reaching
//! crate privates, so the registry has to be built in the library: a
//! `benches/*.rs` is a separate crate and cannot name a `pub(crate)` fn. The
//! runner lives here too, keeping [`run`] and its selection rules
//! unit-testable (a `harness = false` target collects no `#[test]` fns).
//! `benches/criterion.rs` is a three-line call into [`run`]. Allocation gates
//! report counts, not times, and live in `tests/alloc/gates/`.
//!
//! ## Why this owns `main`
//!
//! Criterion's filter gates the `bench_function` call, not the setup a driver
//! runs before it, so under `criterion_main!` filtering to `damage` still paid
//! every other driver's setup (~11 s and two wgpu adapter requests). Selecting
//! on the registry before calling a driver avoids that.
//!
//! ## Delegate-or-own
//!
//! `Criterion::configure_from_args` hard-exits on flags it doesn't know, and
//! `criterion::Mode` is `pub(crate)`, so `Test` and `List` are unreachable.
//! `Mode::Test` is what makes `cargo test --benches` run each benchmark once,
//! and that command does run this binary. So when cargo drives us in test or
//! list mode (signalled by an absence; see `cli::delegates`), argv goes to
//! `configure_from_args` untouched. Otherwise `Cli` parses it and drives
//! criterion's public setters.
//!
//! A driver is handed what the runner decided in a `Run`, not left to
//! re-derive it from argv. Nothing reads the environment; every input is a
//! declared flag. Baselines through `cargo criterion` are untested.

#![expect(
    clippy::print_stdout,
    reason = "listing the bench drivers is the output this command exists for"
)]

mod cli;
mod driver;

use crate::bench::cli::Cli;
use crate::bench::driver::DRIVERS;
use clap::Parser as _;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::env;

/// Which half of the pipeline is in play: on a driver row, what it measures;
/// on the command line, what the run wants. One vocabulary, so selection is an
/// intersection. Single-arm drivers only use it to decide whether they run;
/// the frame bench has both and needs the resolved overlap.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arms {
    /// Touches no GPU.
    Cpu,
    /// Requests a wgpu adapter.
    Gpu,
    Both,
}

impl Arms {
    pub(crate) const fn includes_cpu(self) -> bool {
        matches!(self, Arms::Cpu | Arms::Both)
    }

    pub(crate) const fn includes_gpu(self) -> bool {
        matches!(self, Arms::Gpu | Arms::Both)
    }

    /// What a driver offering `self` should run when the caller asked for
    /// `want`, or `None` when they share nothing.
    pub(crate) const fn overlap(self, want: Arms) -> Option<Arms> {
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
    /// The name the runner selected this driver by, and the namespace of every
    /// id it registers. Reached through [`Run::group`].
    driver: &'static str,
    /// The half of the pipeline to exercise: [`Arms::overlap`] of what the
    /// driver offers against the command line. Single-arm drivers ignore it.
    pub(crate) arms: Arms,
    /// Whether criterion will write `estimates.json`. False in test mode and
    /// under `--profile-time`; a driver reading its numbers back must skip
    /// then.
    pub(crate) recording: bool,
    /// Command-line knobs for a driver that renders the shared fixture and
    /// files a results row (only the frame bench today).
    pub(crate) fixture: Fixture<'a>,
}

impl Run<'_> {
    /// A criterion group named for the driver the runner selected, so a driver
    /// cannot misspell its namespace and renaming a `DRIVERS` row renames its
    /// benchmarks.
    pub(crate) fn group<'c>(&self, c: &'c mut Criterion) -> BenchmarkGroup<'c, WallTime> {
        c.benchmark_group(self.group_name())
    }

    /// [`Self::group`] one level deeper, for a driver measuring several things.
    pub(crate) fn subgroup<'c>(
        &self,
        c: &'c mut Criterion,
        sub: &str,
    ) -> BenchmarkGroup<'c, WallTime> {
        c.benchmark_group(self.subgroup_name(sub))
    }

    pub(crate) const fn group_name(&self) -> &'static str {
        self.driver
    }

    fn subgroup_name(&self, sub: &str) -> String {
        format!("{}/{sub}", self.driver)
    }
}

/// The surface the shared fixture renders into, and the row's caption. `None`
/// means the bench's own default.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fixture<'a> {
    /// `--size <W>x<H>`: the physical surface every arm renders into.
    pub(crate) size: Option<glam::UVec2>,
    /// `--scale`: device pixel ratio.
    pub(crate) scale: Option<f32>,
    /// `--machine`: which per-machine results file the row lands in; defaults
    /// to the short hostname.
    pub(crate) machine: Option<&'a str>,
    /// `--note`: why this was measured.
    pub(crate) note: Option<&'a str>,
}

/// The bench target's entry point.
pub fn run() {
    // Opt-in drivers stay out of test mode: it is a smoke check, and the frame
    // matrix is ~90 s unoptimized.
    let argv: Vec<String> = env::args().collect();
    if cli::delegates(argv.iter().map(String::as_str)) {
        for driver in DRIVERS.iter().filter(|d| !d.opt_in) {
            let mut criterion = (driver.config)().configure_from_args();
            (driver.run)(
                &mut criterion,
                Run {
                    driver: driver.name,
                    arms: Arms::Both,
                    recording: false,
                    fixture: Fixture::default(),
                },
            );
        }
        Criterion::default().configure_from_args().final_summary();
        return;
    }

    let cli = Cli::parse();

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
                driver: driver.name,
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
    use crate::bench::{Arms, Fixture, Run};

    fn run(driver: &'static str) -> Run<'static> {
        Run {
            driver,
            arms: Arms::Both,
            recording: false,
            fixture: Fixture::default(),
        }
    }

    /// A group is the driver's name, a subgroup sits under it, and criterion
    /// joins a leaf with the same separator: `text_atlas` + `encoded_cache` +
    /// `steady` is `text_atlas/encoded_cache/steady`.
    #[test]
    fn group_names_are_the_drivers_namespace() {
        assert_eq!(
            run("text_atlas").subgroup_name("encoded_cache"),
            "text_atlas/encoded_cache",
        );
        assert_eq!(
            run("damage").subgroup_name("region/add"),
            "damage/region/add"
        );
    }

    /// `overlap` is the entire selection rule; `None` is the only skip.
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
