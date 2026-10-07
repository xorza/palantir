//! The bench runner's command line.
//!
//! Criterion's `configure_from_args` **hard-exits on any flag it doesn't know**,
//! so it can't parse an argv carrying ours; the criterion knobs are re-declared
//! and applied through public setters ([`Cli::configure`]).

use crate::bench::driver::Driver;
use crate::bench::{Arms, Fixture};
use clap::Parser;
use criterion::Criterion;
use std::time::Duration;

/// Whether argv is criterion's to parse rather than ours.
///
/// Criterion's rule (`criterion-0.8.2`, `src/lib.rs:960`): `--bench` without
/// `--test` benchmarks, **everything else is test mode**. Cargo passes
/// `--bench` under `cargo bench` and *no arguments* under `cargo test --benches`,
/// so keying on `--test` would make every `cargo test --all-targets` a full run.
///
/// A hand scan, not a lenient `clap` parse: **cargo appends `--bench` after the
/// caller's arguments** and `ignore_errors` stops at the first unrecognised
/// token, so `-d cascade --bench` read as having no `--bench`.
pub(super) fn delegates<'a>(args: impl Iterator<Item = &'a str>) -> bool {
    let mut bench = false;
    for arg in args {
        match arg {
            "--test" | "--list" => return true,
            "--bench" => bench = true,
            _ => {}
        }
    }
    !bench
}

/// Palantir's criterion benchmark drivers.
#[derive(Parser, Debug)]
#[command(
    name = "palantir-bench",
    about = "Run palantir's criterion benchmark drivers",
    disable_version_flag = true
)]
pub(super) struct Cli {
    /// Regex over benchmark ids, applied within the selected drivers.
    filter: Option<String>,

    /// Run only these drivers, by exact name. Repeatable. Naming an opt-in driver
    /// opts it in.
    #[arg(short = 'd', long = "driver", value_name = "NAME")]
    drivers: Vec<String>,

    /// Which half of the pipeline to measure. `cpu` runs no driver that
    /// requests a wgpu adapter.
    #[arg(long, value_enum, default_value_t = Arms::Both)]
    pub(super) arms: Arms,

    /// Print the driver names and exit.
    #[arg(long)]
    pub(super) list_drivers: bool,

    /// Physical surface every arm renders into, e.g. `3840x6000`.
    #[arg(long, value_name = "WxH", value_parser = parse_size)]
    size: Option<glam::UVec2>,
    /// Device pixel ratio the fixture renders at.
    #[arg(long, value_name = "DPR")]
    scale: Option<f32>,
    /// Which per-machine results file the row lands in. Defaults to the
    /// short hostname.
    #[arg(long, value_name = "NAME")]
    machine: Option<String>,
    /// Context recorded alongside the frame bench's results row.
    #[arg(long, value_name = "TEXT")]
    note: Option<String>,

    /// Profile for this many seconds per benchmark instead of sampling.
    #[arg(long, value_name = "SECONDS")]
    profile_time: Option<f64>,
    /// Samples criterion collects per benchmark. Fewer is faster and
    /// noisier; criterion's own default is 100.
    #[arg(long, value_name = "N")]
    sample_size: Option<usize>,
    /// Seconds criterion spends collecting those samples.
    #[arg(long, value_name = "SECONDS")]
    measurement_time: Option<f64>,
    /// Seconds criterion runs before measuring, so caches and the clock settle.
    #[arg(long, value_name = "SECONDS")]
    warm_up_time: Option<f64>,
    /// Store this run's samples under a name, for a later `--baseline`
    /// to compare against.
    #[arg(long, value_name = "NAME")]
    save_baseline: Option<String>,
    /// Compare against a named baseline rather than the previous run.
    /// Fails if a selected benchmark has no sample under that name.
    #[arg(
        short = 'b',
        long,
        value_name = "NAME",
        conflicts_with = "save_baseline"
    )]
    baseline: Option<String>,
    /// [`Self::baseline`], except a benchmark with no sample under that
    /// name is left uncompared instead of failing the run.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["save_baseline", "baseline"])]
    baseline_lenient: Option<String>,
    /// Disable plot and HTML generation.
    #[arg(long)]
    noplot: bool,

    /// Cargo passes this to every `harness = false` target. Accepted and ignored.
    #[arg(long, hide = true)]
    bench: bool,
}

/// `<W>x<H>` in physical pixels, a `value_parser` so a malformed size is a clap
/// error next to the flag.
fn parse_size(raw: &str) -> Result<glam::UVec2, String> {
    let (w, h) = raw
        .trim()
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected <width>x<height>, got {raw:?}"))?;
    let axis = |s: &str, which: &str| {
        s.trim()
            .parse::<u32>()
            .map_err(|e| format!("{which} in {raw:?}: {e}"))
    };
    Ok(glam::UVec2::new(axis(w, "width")?, axis(h, "height")?))
}

impl Cli {
    pub(super) fn fixture(&self) -> Fixture<'_> {
        Fixture {
            size: self.size,
            scale: self.scale,
            machine: self.machine.as_deref(),
            note: self.note.as_deref(),
        }
    }

    /// Whether criterion will write `estimates.json` this run. Profile mode writes
    /// nothing ("Analysis Disabled").
    pub(super) const fn records(&self) -> bool {
        self.profile_time.is_none()
    }

    /// Every name given to `--driver` must exist, or the run silently measures less.
    pub(super) fn validate(&self, known: &[Driver]) {
        for name in &self.drivers {
            assert!(
                known.iter().any(|d| d.name == name),
                "unknown driver {name:?}; try --list-drivers",
            );
        }
    }

    /// A bare run reaches every driver except the opt-in ones; naming any driver
    /// switches to that list verbatim, opt-in included.
    pub(super) fn selects(&self, driver: &Driver) -> bool {
        if self.drivers.is_empty() {
            !driver.opt_in
        } else {
            self.drivers.iter().any(|n| n == driver.name)
        }
    }

    /// Apply the parsed knobs to a driver's base configuration, standing in for
    /// `configure_from_args`.
    pub(super) fn configure(&self, mut c: Criterion) -> Criterion {
        if let Some(f) = &self.filter {
            c = c.with_filter(f);
        }
        if let Some(s) = self.profile_time {
            c = c.profile_time(Some(Duration::from_secs_f64(s)));
        }
        if let Some(n) = self.sample_size {
            c = c.sample_size(n);
        }
        if let Some(s) = self.measurement_time {
            c = c.measurement_time(Duration::from_secs_f64(s));
        }
        if let Some(s) = self.warm_up_time {
            c = c.warm_up_time(Duration::from_secs_f64(s));
        }
        if let Some(b) = &self.save_baseline {
            c = c.save_baseline(b.clone());
        }
        // `strict` distinguishes the two flags; clap rules out both being set.
        if let Some(b) = &self.baseline {
            c = c.retain_baseline(b.clone(), true);
        }
        if let Some(b) = &self.baseline_lenient {
            c = c.retain_baseline(b.clone(), false);
        }
        if self.noplot {
            c = c.without_plots();
        }
        c
    }
}

#[cfg(test)]
mod tests;
