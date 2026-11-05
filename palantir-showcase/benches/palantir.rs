//! Every criterion bench of palantir, in one target: `cargo bench-all`.
//!
//! The drivers live in palantir's library (`palantir::bench`): each sits in a `bench.rs` beside the
//! code it measures and reaches crate privates. They run over the showcase's first page, which is
//! why the target lives here, as palantir cannot depend on the crate that depends on it.

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::bench;
use palantir_showcase::bench::Showcase;

fn all(c: &mut Criterion) {
    bench::run::<Showcase>(c);
}

criterion_group!(benches, all);
criterion_main!(benches);
