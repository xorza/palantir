//! What each GPU atlas holds over the showcase's first page and the backdrop scenes:
//! `cargo atlas-memory`.

use palantir::bench;
use palantir_showcase::bench::Showcase;
use std::process::ExitCode;

fn main() -> ExitCode {
    bench::atlas_memory::<Showcase>()
}
