//! The bench runner's command line.
//!
//! Criterion parses argv itself in `configure_from_args`, which builds
//! its own `clap::Command` and **hard-exits on any flag it doesn't
//! know** — so it cannot be called on an argv carrying ours. That is why
//! the criterion knobs below are re-declared and applied through public
//! setters ([`Cli::configure`]) rather than forwarded.

use crate::bench::driver::Driver;
use crate::bench::{Arms, Fixture};
use clap::Parser;
use criterion::Criterion;
use std::time::Duration;

/// Whether argv is criterion's to parse rather than ours.
///
/// Criterion's rule, from its own `configure_from_args`
/// (`criterion-0.8.2`, `src/lib.rs:960`): `--bench` without `--test`
/// benchmarks, **everything else is test mode**. So test mode is
/// signalled by an absence — cargo passes a `harness = false` bench
/// target `--bench` under `cargo bench` and *no arguments at all* under
/// `cargo test --benches`. Keying on a `--test` that cargo never sends
/// turns every `cargo test --all-targets` into a full measurement run.
///
/// A hand scan rather than a lenient `clap` parse, which cannot do this
/// job: **cargo appends `--bench` after the caller's own arguments**,
/// and `ignore_errors` stops collecting at the first token it doesn't
/// recognise. A `Gate` deriving `Parser` therefore read
/// `-d cascade --bench` as having no `--bench` at all and handed the
/// whole run to criterion, which then rejected `-d`. Every
/// `cargo bench -- <anything>` broke that way; only the bare
/// `cargo bench` survived, because its argv is `--bench` alone.
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

    /// Run only these drivers, by exact name. Repeatable. Naming an
    /// opt-in driver is what opts it in.
    #[arg(short = 'd', long = "driver", value_name = "NAME")]
    drivers: Vec<String>,

    /// Which half of the pipeline to measure. `cpu` runs no driver that
    /// requests a wgpu adapter.
    #[arg(long, value_enum, default_value_t = Arms::Both)]
    pub(super) arms: Arms,

    /// Print the driver names and exit.
    #[arg(long)]
    pub(super) list_drivers: bool,

    // ── knobs for a driver that renders the shared fixture; see
    // `Fixture`. Declared here because this is the only parser ──
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

    // ── criterion's own knobs, re-declared because we drive its setters
    // rather than letting it parse argv ──
    /// Profile for this many seconds per benchmark instead of sampling.
    #[arg(long, value_name = "SECONDS")]
    profile_time: Option<f64>,
    #[arg(long, value_name = "N")]
    sample_size: Option<usize>,
    #[arg(long, value_name = "SECONDS")]
    measurement_time: Option<f64>,
    #[arg(long, value_name = "SECONDS")]
    warm_up_time: Option<f64>,
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

    /// Cargo passes this to every `harness = false` target. Accepted and
    /// ignored — on this path criterion never sees argv at all.
    #[arg(long, hide = true)]
    bench: bool,
}

/// `<W>x<H>` in physical pixels. A `value_parser` rather than a parse
/// at the use site: a malformed size should be a clap error next to the
/// flag, not a panic partway into a bench.
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
    pub(super) fn parse_args() -> Self {
        Cli::parse()
    }

    pub(super) fn fixture(&self) -> Fixture<'_> {
        Fixture {
            size: self.size,
            scale: self.scale,
            machine: self.machine.as_deref(),
            note: self.note.as_deref(),
        }
    }

    /// Whether criterion will write `estimates.json` this run. Profile
    /// mode reports "Analysis Disabled" and writes nothing, so a driver
    /// that reads its own numbers back has to know.
    pub(super) fn records(&self) -> bool {
        self.profile_time.is_none()
    }

    /// Every name given to `--driver` must exist, or the run silently
    /// measures less than asked for.
    pub(super) fn validate(&self, known: &[Driver]) {
        for name in &self.drivers {
            assert!(
                known.iter().any(|d| d.name == name),
                "unknown driver {name:?}; try --list-drivers",
            );
        }
    }

    /// A bare run reaches every driver except the opt-in ones — those are
    /// exactly the ones that shouldn't happen by accident. Naming any
    /// driver switches to that list verbatim, opt-in included.
    pub(super) fn selects(&self, driver: &Driver) -> bool {
        if self.drivers.is_empty() {
            !driver.opt_in
        } else {
            self.drivers.iter().any(|n| n == driver.name)
        }
    }

    /// Apply the parsed knobs to a driver's base configuration. Stands in
    /// for `configure_from_args`, which cannot be used here.
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
        // `strict` is the difference between the two flags, and clap has
        // already ruled out both being set.
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
