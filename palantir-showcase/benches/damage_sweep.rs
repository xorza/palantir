//! What each damage decision costs over the showcase's first page, and how far each policy lands
//! from the cheapest: `cargo damage-sweep`. `palantir-showcase/benches/AGENTS.md` says how to read it.

use palantir::bench;
use palantir_showcase::bench::Showcase;
use std::process::ExitCode;

fn main() -> ExitCode {
    bench::damage_sweep::<Showcase>()
}
