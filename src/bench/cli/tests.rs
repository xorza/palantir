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
