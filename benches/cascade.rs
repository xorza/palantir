//! Cascade-pass microbenchmark. Builds a synthetic flat tree with N
//! nodes, runs `Ui::post_record` once to populate `layout.results`, then
//! benches `CascadesEngine::run` in isolation.
//!
//! Decision criterion (per `docs/tree-redesign.md` Phase 2):
//!
//! - cascade < 0.1 ms on N=2000 → field split (Phase 3) is theater;
//!   defer indefinitely.
//! - cascade > 1 ms on N=2000 → ship the field split.
//! - in between → judgement call.
//!
//! `Ui::for_test()` leaves the cosmic shaper unset, so text measurement
//! runs through the mono fallback (same as `frame.rs`/`caches.rs`).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use palantir::{Configure, Display, Frame, FrameStamp, Panel, Sizing, Ui};
use std::hint::black_box;

/// Build a flat tree of `n` leaves under a single VStack root.
/// Approximates a long list of items — the wide-and-shallow shape that
/// stresses the cascade pre-order walk hardest because every leaf
/// pushes a fresh `HitEntry`.
fn build_flat(ui: &mut Ui, n: usize) {
    Panel::vstack()
        .id_salt("root")
        .gap(2.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for i in 0..n {
                Frame::new()
                    .id_salt(("row", i))
                    .size((Sizing::FILL, Sizing::Fixed(20.0)))
                    .show(ui);
            }
        });
}

fn bench_cascade(c: &mut Criterion) {
    let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
    let mut group = c.benchmark_group("cascade/run");

    for &n in &[100usize, 500, 2000, 10_000] {
        // Build once, post_record once to populate layout.results, then
        // measure cascades.run in isolation.
        let mut ui = Ui::for_test();
        let _ = ui.frame(
            FrameStamp::new(display, std::time::Duration::ZERO),
            &mut (),
            |ui| build_flat(ui, n),
        );

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                ui.run_cascades();
                black_box(&ui);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_cascade);
criterion_main!(benches);
