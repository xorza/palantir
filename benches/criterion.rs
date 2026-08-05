//! Every criterion driver in the crate, in one target.
//!
//! Select with criterion's own filter rather than a target name:
//!
//! ```sh
//! cargo bench -p palantir --bench criterion -- damage
//! cargo bench -p palantir --bench criterion -- 'cascade/hit_test$'
//! ```
//!
//! Every benchmark id is namespaced by subsystem (`damage/workload`,
//! `frame/cached_cpu`, …), so a bare subsystem name selects that
//! driver's whole set.
//!
//! One target rather than eighteen because `[profile.bench]` is fat-LTO
//! with one codegen unit: each bench binary links the entire dependency
//! graph, and cargo links every target in parallel. Eighteen of those at
//! once is the OOM this crate's `AGENTS.md` used to warn about. The
//! three `alloc_*` targets stay separate — each installs `dhat::Alloc`
//! as its `#[global_allocator]`, which is per-binary and would distort
//! every timing here.
//!
//! `frame` opts in through `PALANTIR_BENCH_MODE` (see
//! `palantir::bench::frame`); without it the driver prints a notice and
//! returns, so a run that asked for something else doesn't pay its
//! ~90 s matrix.

use criterion::{criterion_group, criterion_main};
use palantir::bench;

// Its own group so `bench::frame_config`'s longer measurement window
// applies to the frame arms and to nothing else.
criterion_group! {
    name = frame;
    config = bench::frame_config();
    targets = bench::frame
}

criterion_group!(
    rest,
    bench::animation,
    bench::cascade,
    bench::composer,
    bench::curve_pipeline,
    bench::damage,
    bench::gradient,
    bench::gradient_atlas,
    bench::image_pipeline,
    bench::input,
    bench::layout_caches,
    bench::paint_anims,
    bench::record_pass,
    bench::schedule,
    bench::text_atlas,
    bench::text_edit,
    bench::text_grid,
    bench::text_shape,
);

criterion_main!(frame, rest);
