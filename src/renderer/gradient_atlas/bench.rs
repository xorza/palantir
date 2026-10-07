//! Register-path scaling benchmark for the gradient LUT atlas.
//!
//! Tests **flatness, not speed**: `register` must not get more expensive as
//! the atlas grows. Each arm runs at two capacities; compare the ratio
//! between them from a single quiet run, not absolutes (256 rows stay
//! cache-resident, 2048 spill). `miss/*` is the arm to watch for per-row walks.
//!
//! Both arms hold the working set fixed and vary only the table size;
//! scaling the working set with capacity confounds lookup cost with memory
//! touched.
//!
//! - `hit/*`: a fixed [`WORKING_SET`] of resident gradients re-registered, as
//!   a frame redrawing unchanged chrome does.
//! - `miss/*`: a never-seen gradient every iteration, so it misses, evicts and
//!   re-bakes regardless of table size.
//!
//! Both assert against
//! [`GradientAtlasCounters`](super::counters::GradientAtlasCounters) first, so
//! a fixture that stops doing what its name says fails loudly.
//!
//! Run with `cargo bench --features bench --bench criterion -- gradient_atlas`.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::common::counters::CounterSet;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::gradient_atlas::CpuGradientAtlas;
use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

/// Capacities compared: the initial size and three doublings up.
const CAPACITIES: [u32; 2] = [256, 2048];

/// Resident gradients the `hit` arms cycle through; fixed across capacities.
const WORKING_SET: u32 = 128;

const CHURN_BASE: u32 = 1_000_000;

/// Distinct ramp per seed, walking the colour cube so every seed is a distinct
/// bake key.
///
/// Deliberately structured, not pre-mixed: constant channels across a run are
/// what a themed palette looks like and what `GradientStops::hash` must
/// spread, so this also guards that hash. Built from sRGB bytes, which
/// round-trip exactly.
fn gradient_for(seed: u32) -> ColorRamp {
    let a = SrgbaU8::rgb(seed as u8, (seed >> 8) as u8, (seed >> 16) as u8);
    let b = SrgbaU8::rgb((seed >> 4) as u8, (seed >> 12) as u8, 0x40);
    ColorRamp::new([
        Stop::new(0.0, RgbaF32::from_srgba(a)),
        Stop::new(1.0, RgbaF32::from_srgba(b)),
    ])
}

/// Atlas grown to `capacity` and filled to one row short of full.
///
/// Fills in a single epoch so the table grows rather than evicting, then
/// `flush` frees the rows for eviction.
fn filled(capacity: u32) -> CpuGradientAtlas {
    let mut atlas = CpuGradientAtlas::new(capacity);
    let mut seed = 0u32;
    while atlas.capacity() < capacity || atlas.counters.counts().bakes < capacity - 1 {
        seed += 1;
        atlas.register(&gradient_for(seed));
        assert!(seed < capacity * 4, "fill made no progress");
    }
    assert_eq!(atlas.capacity(), capacity);
    atlas.flush();
    atlas
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut group = run.subgroup(c, "register");
    group.sample_size(50);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_secs(2));
    group.throughput(Throughput::Elements(1));

    for capacity in CAPACITIES {
        // Steady state: the most recent `WORKING_SET` rows are resident at any capacity.
        let mut atlas = filled(capacity);
        let resident: Vec<ColorRamp> = (capacity - WORKING_SET..capacity)
            .map(gradient_for)
            .collect();
        let before = atlas.counters.counts().bakes;
        for ramp in &resident {
            black_box(atlas.register(ramp));
        }
        assert_eq!(
            atlas.counters.counts().bakes,
            before,
            "hit/{capacity} fixture re-baked: the working set is not resident",
        );

        let mut i = 0usize;
        group.bench_with_input(BenchmarkId::new("hit", capacity), &capacity, |b, _| {
            b.iter(|| {
                let ramp = &resident[i % resident.len()];
                i = i.wrapping_add(1);
                black_box(atlas.register(ramp))
            });
        });

        // Churn: a never-registered gradient every iteration, so a miss is
        // independent of table size.
        let mut atlas = filled(capacity);
        let (hits, bakes) = (atlas.counters.counts().hits, atlas.counters.counts().bakes);
        for k in 0..16 {
            atlas.flush();
            black_box(atlas.register(&gradient_for(CHURN_BASE + k)));
        }
        assert_eq!(
            atlas.counters.counts().hits,
            hits,
            "miss/{capacity} fixture hit the index: the churn seeds overlap the fill",
        );
        assert_eq!(
            atlas.counters.counts().bakes - bakes,
            16,
            "miss/{capacity} fixture must bake exactly once per registration",
        );

        let growths = atlas.counters.counts().growths;
        let mut seed = CHURN_BASE + 16;
        group.bench_with_input(BenchmarkId::new("miss", capacity), &capacity, |b, _| {
            b.iter(|| {
                // `flush` ends the epoch; otherwise claimed rows stay eviction-exempt and
                // the atlas grows instead of churning.
                atlas.flush();
                seed = seed.wrapping_add(1);
                black_box(atlas.register(&gradient_for(seed)))
            });
        });
        assert_eq!(
            atlas.counters.counts().growths,
            growths,
            "miss/{capacity} grew the atlas: it measured a ratchet, not churn",
        );
        eprintln!(
            "[gradient_atlas] capacity={capacity} rows={} evictions={} fallbacks={}",
            atlas.capacity(),
            atlas.counters.counts().evictions,
            atlas.counters.counts().fallbacks,
        );
    }
    group.finish();
}
