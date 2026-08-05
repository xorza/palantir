//! The bench runner's command line.
//!
//! Criterion parses argv itself in `configure_from_args`, which builds
//! its own `clap::Command` and **hard-exits on any flag it doesn't
//! know** — so it cannot be called on an argv carrying ours. That is why
//! the criterion knobs below are re-declared and applied through public
//! setters ([`Cli::configure`]) rather than forwarded.

use crate::bench::Arms;
use crate::bench::driver::Driver;
use clap::Parser;
use criterion::Criterion;
use std::time::Duration;

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
    #[arg(long, value_enum, default_value_t = ArmsArg::Both)]
    arms: ArmsArg,

    /// Print the driver names and exit.
    #[arg(long)]
    pub(super) list_drivers: bool,

    /// Context recorded alongside the frame bench's results row.
    #[arg(long, value_name = "TEXT")]
    pub(super) note: Option<String>,

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
    /// Disable plot and HTML generation.
    #[arg(long)]
    noplot: bool,

    /// Cargo passes this to every `harness = false` target. Accepted and
    /// ignored — on this path criterion never sees argv at all.
    #[arg(long, hide = true)]
    bench: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum ArmsArg {
    Cpu,
    Gpu,
    Both,
}

impl From<ArmsArg> for Arms {
    fn from(a: ArmsArg) -> Arms {
        match a {
            ArmsArg::Cpu => Arms::Cpu,
            ArmsArg::Gpu => Arms::Gpu,
            ArmsArg::Both => Arms::Both,
        }
    }
}

impl Cli {
    pub(super) fn parse_args() -> Self {
        Cli::parse()
    }

    pub(super) fn arms(&self) -> Arms {
        self.arms.into()
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
        if self.noplot {
            c = c.without_plots();
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use crate::bench::cli::Cli;
    use crate::bench::driver::DRIVERS;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        let mut argv = vec!["palantir-bench"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).expect("parse")
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
        assert_eq!(parse(&[]).arms(), Arms::Both);
        assert_eq!(parse(&["--arms", "cpu"]).arms(), Arms::Cpu);
        assert_eq!(parse(&["--arms", "gpu"]).arms(), Arms::Gpu);
        assert!(Cli::try_parse_from(["palantir-bench", "--arms", "nope"]).is_err());
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
