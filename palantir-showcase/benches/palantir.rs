//! Every bench of palantir, in one target: the criterion cases (`cargo bench-all`) and the studies
//! its first argument names (`cargo damage-sweep`, `cargo atlas-memory`), so one release link
//! serves them all.
//!
//! The drivers live in palantir's library (`palantir::bench`): each sits in a `bench.rs` beside the
//! code it measures and reaches crate privates. They run over the showcase's first page, which is
//! why the target lives here, as palantir cannot depend on the crate that depends on it.

use palantir::bench;
use palantir_showcase::bench::Showcase;
use std::process::ExitCode;

fn main() -> ExitCode {
    bench::main::<Showcase>()
}
