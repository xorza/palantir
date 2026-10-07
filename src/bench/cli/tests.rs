use crate::bench::cli::{Cli, delegates};
use crate::bench::driver::DRIVERS;
use clap::Parser;

fn parse(args: &[&str]) -> Cli {
    let mut argv = vec!["palantir-bench"];
    argv.extend_from_slice(args);
    Cli::try_parse_from(argv).expect("parse")
}

/// The argv each cargo invocation sends, checked against a throwaway `harness = false` target. `--bench` comes last, as
/// cargo appends it after the caller's arguments; tests that put it first prove nothing.
#[test]
fn cargos_argv_routes_to_us_and_a_bare_one_delegates() {
    let d = |args: &[&str]| delegates(args.iter().copied());

    assert!(d(&[]));
    assert!(!d(&["--bench"]));
    assert!(!d(&["-d", "damage", "--bench"]));
    assert!(!d(&["cascade/hit_test", "--save-baseline", "x", "--bench"]));
    assert!(!d(&["--arms", "cpu", "--profile-time", "2", "--bench"]));
    // `cargo bench -- --test` and `--list` are criterion's own modes; it parses them.
    assert!(d(&["--test", "--bench"]));
    assert!(d(&["--list", "--bench"]));
    assert!(!d(&["test", "--bench"]));
}

/// `--save-baseline` writes a sample and the compare flags read one back; saving and comparing in one run would
/// overwrite what is measured against, so it is rejected.
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

/// Profile mode disables criterion's analysis, so no `estimates.json` is written for the frame bench.
#[test]
fn only_a_sampling_run_records_estimates() {
    assert!(parse(&["--bench"]).records());
    assert!(parse(&["--bench", "-d", "frame"]).records());
    assert!(!parse(&["--bench", "--profile-time", "5"]).records());
}

/// A bare run reaches every ordinary driver and no opt-in one; a named run reaches exactly what it named.
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

/// `--arms` is the one axis shared with the driver rows; a typo must not silently widen or narrow a run.
#[test]
fn arms_parses_and_defaults_to_both() {
    use crate::bench::Arms;
    assert_eq!(parse(&[]).arms, Arms::Both);
    assert_eq!(parse(&["--arms", "cpu"]).arms, Arms::Cpu);
    assert_eq!(parse(&["--arms", "gpu"]).arms, Arms::Gpu);
    assert!(Cli::try_parse_from(["palantir-bench", "--arms", "nope"]).is_err());
}

/// The fixture knobs are flags, so the parse is the only place a malformed one is caught.
#[test]
fn fixture_knobs_parse_and_reject_junk() {
    assert_eq!(parse(&[]).fixture().size, None);
    let cli = parse(&["--size", "1920x1080", "--scale", "1.5", "--machine", "rig"]);
    let fx = cli.fixture();
    assert_eq!(fx.size, Some(glam::UVec2::new(1920, 1080)));
    assert_eq!(fx.scale, Some(1.5));
    assert_eq!(fx.machine, Some("rig"));
    // `X` is accepted as the separator; a missing or non-numeric axis is a clap error, not a mid-bench panic.
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

/// Cargo passes `--bench` to every `harness = false` target and a bare filter is criterion's positional; rejecting either breaks `cargo bench`.
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
