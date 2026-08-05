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
mod tests {
    use crate::bench::cli::{Cli, delegates};
    use crate::bench::driver::DRIVERS;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        let mut argv = vec!["palantir-bench"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).expect("parse")
    }

    /// The argv each cargo invocation actually sends, verified against a
    /// throwaway `harness = false` target.
    ///
    /// **`--bench` comes last.** Cargo appends it *after* the caller's
    /// own arguments, so every case below puts it where cargo does —
    /// the ordering an earlier `clap`-based gate got wrong, reading
    /// `-d damage --bench` as having no `--bench` and handing the run to
    /// criterion. Tests that put it first pass either way and prove
    /// nothing.
    #[test]
    fn cargos_argv_routes_to_us_and_a_bare_one_delegates() {
        let d = |args: &[&str]| delegates(args.iter().copied());

        // `cargo test --benches` sends nothing at all.
        assert!(d(&[]));
        // `cargo bench` and `cargo bench -- <args>`.
        assert!(!d(&["--bench"]));
        assert!(!d(&["-d", "damage", "--bench"]));
        assert!(!d(&["cascade/hit_test", "--save-baseline", "x", "--bench"]));
        assert!(!d(&["--arms", "cpu", "--profile-time", "2", "--bench"]));
        // `cargo bench -- --test` is criterion's own "run them once",
        // and `--list` is its enumeration mode. Both are its to parse.
        assert!(d(&["--test", "--bench"]));
        assert!(d(&["--list", "--bench"]));
        // A positional filter is not a flag and must not delegate.
        assert!(!d(&["test", "--bench"]));
    }

    /// `--save-baseline` writes a named sample; the two compare flags
    /// read one back. Saving and comparing in the same run is
    /// contradictory — criterion rules it out and so do we, or a run
    /// would silently overwrite the thing it was asked to measure
    /// against.
    #[test]
    fn baseline_flags_parse_and_exclude_each_other() {
        assert_eq!(parse(&["-b", "before"]).baseline.as_deref(), Some("before"));
        assert_eq!(
            parse(&["--baseline", "before"]).baseline.as_deref(),
            Some("before"),
        );
        assert_eq!(
            parse(&["--baseline-lenient", "before"])
                .baseline_lenient
                .as_deref(),
            Some("before"),
        );
        for clash in [
            &["--save-baseline", "a", "--baseline", "a"][..],
            &["--save-baseline", "a", "--baseline-lenient", "a"],
            &["--baseline", "a", "--baseline-lenient", "a"],
        ] {
            let mut argv = vec!["palantir-bench"];
            argv.extend_from_slice(clash);
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{clash:?} must be rejected",
            );
        }
    }

    /// Profile mode disables criterion's analysis, so nothing writes an
    /// `estimates.json` for the frame bench to read back.
    #[test]
    fn only_a_sampling_run_records_estimates() {
        assert!(parse(&["--bench"]).records());
        assert!(parse(&["--bench", "-d", "frame"]).records());
        assert!(!parse(&["--bench", "--profile-time", "5"]).records());
    }

    /// The selection rule, which decides what a run actually measures.
    /// A bare run must reach every ordinary driver and no opt-in one; a
    /// named run must reach exactly what was named, opt-in included —
    /// naming it *is* the opt-in.
    #[test]
    fn bare_run_skips_opt_in_and_named_run_takes_exactly_what_it_named() {
        let named = |cli: &Cli| -> Vec<&'static str> {
            DRIVERS
                .iter()
                .filter(|d| cli.selects(d))
                .map(|d| d.name)
                .collect()
        };

        let bare = named(&parse(&[]));
        assert!(!bare.contains(&"frame"), "bare run must skip opt-in");
        assert_eq!(
            bare.len(),
            DRIVERS.len() - 1,
            "bare run must reach every other driver",
        );

        assert_eq!(named(&parse(&["-d", "frame"])), ["frame"]);
        assert_eq!(named(&parse(&["-d", "damage"])), ["damage"]);
        assert_eq!(
            named(&parse(&["-d", "damage", "-d", "cascade"])),
            ["cascade", "damage"],
            "order follows the table, not the command line",
        );
    }

    /// `--arms` is the one axis shared with the driver rows; a typo must
    /// not silently widen or narrow a run.
    #[test]
    fn arms_parses_and_defaults_to_both() {
        use crate::bench::Arms;
        assert_eq!(parse(&[]).arms, Arms::Both);
        assert_eq!(parse(&["--arms", "cpu"]).arms, Arms::Cpu);
        assert_eq!(parse(&["--arms", "gpu"]).arms, Arms::Gpu);
        assert!(Cli::try_parse_from(["palantir-bench", "--arms", "nope"]).is_err());
    }

    /// The fixture knobs were environment variables read deep inside the
    /// frame bench; they are flags now, so the parse is the only place
    /// a malformed one can be caught.
    #[test]
    fn fixture_knobs_parse_and_reject_junk() {
        assert_eq!(parse(&[]).fixture().size, None);
        let cli = parse(&["--size", "1920x1080", "--scale", "1.5", "--machine", "rig"]);
        let fx = cli.fixture();
        assert_eq!(fx.size, Some(glam::UVec2::new(1920, 1080)));
        assert_eq!(fx.scale, Some(1.5));
        assert_eq!(fx.machine, Some("rig"));
        // `X` is accepted as the separator; a missing or non-numeric
        // axis is a clap error, not a panic mid-bench.
        assert_eq!(
            parse(&["--size", "800X600"]).fixture().size,
            Some(glam::UVec2::new(800, 600)),
        );
        for junk in [
            &["--size", "1920"][..],
            &["--size", "axb"],
            &["--size", "1920x"],
        ] {
            let mut argv = vec!["palantir-bench"];
            argv.extend_from_slice(junk);
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{junk:?} must be rejected"
            );
        }
    }

    /// Cargo passes `--bench` to every `harness = false` target, and a
    /// bare filter is criterion's own positional. Rejecting either would
    /// break `cargo bench` outright.
    #[test]
    fn accepts_cargos_bench_flag_and_a_positional_filter() {
        assert!(parse(&["--bench"]).drivers.is_empty());
        let cli = parse(&["--bench", "cascade/hit_test"]);
        assert_eq!(cli.filter.as_deref(), Some("cascade/hit_test"));
    }

    #[test]
    #[should_panic(expected = "unknown driver")]
    fn an_unknown_driver_name_is_rejected() {
        parse(&["-d", "no_such_driver"]).validate(DRIVERS);
    }
}
