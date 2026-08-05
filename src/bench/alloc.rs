//! The allocation bench's entry point and command line.
//!
//! No driver registry here, unlike the criterion runner next door: this
//! target is one bench of a few steps, all of which answer the same
//! question, so there is nothing to select between. See
//! [`crate::host::bench`] for the steps.

use crate::host::bench;
use clap::Parser;

/// Palantir's dhat allocation bench.
#[derive(Parser, Debug)]
#[command(
    name = "palantir-alloc",
    about = "Run palantir's dhat allocation bench",
    disable_version_flag = true
)]
struct Cli {
    /// Swap the counting-only profiler for the heap profiler, which
    /// writes `dhat-heap.json` on exit — load it at
    /// <https://nnethercote.github.io/dh_view/>.
    #[arg(long)]
    dump: bool,

    /// Cargo passes this to every `harness = false` target. Accepted and
    /// ignored.
    #[arg(long, hide = true)]
    bench: bool,
}

/// The alloc target's entry point.
///
/// No delegate-or-own branch, unlike the criterion runner: there is no
/// criterion here to hand argv to, and cargo's `cargo test --benches`
/// argv — nothing at all — already parses to "every step, counting
/// only", which is the smoke check that mode wants.
pub fn run() {
    bench::alloc(Cli::parse().dump);
}

#[cfg(test)]
mod tests {
    use crate::bench::alloc::Cli;
    use clap::Parser;

    /// The two argvs cargo actually sends — `--bench` under
    /// `cargo bench`, nothing under `cargo test --benches` — must both
    /// parse to a plain counting run.
    #[test]
    fn cargos_argv_parses_and_dump_is_opt_in() {
        let parse = |args: &[&str]| {
            let mut argv = vec!["palantir-alloc"];
            argv.extend_from_slice(args);
            Cli::try_parse_from(argv).expect("parse")
        };
        assert!(!parse(&[]).dump);
        assert!(!parse(&["--bench"]).dump);
        assert!(parse(&["--bench", "--dump"]).dump);
        assert!(Cli::try_parse_from(["palantir-alloc", "-d", "free"]).is_err());
    }
}
