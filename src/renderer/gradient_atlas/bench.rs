//! Register-path scaling benchmark for the gradient LUT atlas.
//!
//! The property under test is **flatness, not speed**: `register_stops`
//! must not get more expensive as the atlas grows. It used to — lookup
//! was an open-addressed probe over the row table whose eviction arm
//! broke the probe invariant, so correctness required scanning every
//! row and a miss cost O(capacity) twice over (the probe sweep plus an
//! LRU scan). Capacity only ever grows, so one gradient-heavy frame
//! made that permanent.
//!
//! Each arm runs at two capacities and the reading that matters is the
//! *ratio between them*. `miss/*` is the arm that carried the cliff and
//! is the one to watch: a regression reintroducing any per-row walk
//! shows up there as `2048` diverging from `256` long before it looks
//! wrong in isolation.
//!
//! Read the absolute numbers with care. Both arms stay resident in
//! cache at 256 rows and spill at 2048, so a capacity-shaped term
//! survives even with the algorithm flat, and all four are sensitive to
//! whatever else is loading the machine's memory bandwidth. Compare
//! ratios from a single quiet run; do not compare absolutes across
//! runs.
//!
//! **Both arms hold the working set fixed and vary only the table
//! size**, which is what isolates the claim. Scaling the working set
//! with the capacity — the obvious way to write this — confounds
//! "lookup got slower" with "the fixture touches 8× more memory", and
//! reports ~1.9× for an algorithm that didn't change.
//!
//! Two workloads:
//!
//! - `hit/*` — a fixed [`WORKING_SET`] of already-resident gradients,
//!   re-registered the way a frame redrawing unchanged chrome does. The
//!   steady-state path, and the one that used to degrade with load
//!   factor (1.1 probes at 25 % occupancy, 11.6 at 99 %). The table is
//!   filled to one row short of full either way, so clustering would be
//!   at its worst.
//! - `miss/*` — a gradient never seen before on every single iteration,
//!   so it misses, evicts, and re-bakes regardless of table size. This
//!   is what a per-frame animated gradient produces, and the arm the
//!   old O(capacity) probe-plus-LRU-scan dominated.
//!
//! Both arms assert against [`GradientAtlasProbe`] before measuring, so
//! a fixture that quietly stopped doing what its name says fails loudly
//! instead of reporting a plausible-looking time. `miss/*` in particular
//! would read as a *speedup* if it started hitting.
//!
//! Requires the `internals` feature. Run with
//! `cargo bench --features bench --bench criterion -- gradient_atlas`.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::color::ColorU8;
use crate::renderer::gradient_atlas::CpuGradientAtlas;
use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

/// Capacities compared. 256 is the initial size; 2048 is three
/// doublings up, which the old probe made 8× more expensive per miss.
const CAPACITIES: [u32; 2] = [256, 2048];

/// Resident gradients the `hit` arms cycle through. Fixed across
/// capacities on purpose — see the module doc.
const WORKING_SET: u32 = 128;

/// Seed base for the `miss` arms, past anything [`filled`] uses.
const CHURN_BASE: u32 = 1_000_000;

/// Distinct stop sequence per seed, walking the colour cube directly so
/// every seed is a distinct bake key.
///
/// Deliberately *structured* rather than pre-mixed: whole channels stay
/// constant across a run, which is what a real themed palette looks
/// like and what `GradientStops::hash` has to spread. These arms ran at
/// 232 ns and 615 ns per hit while that hash packed colour into the high
/// half of its word; keeping the naive fixture makes them an end-to-end
/// guard on the layout as well as on the atlas.
fn gradient_for(seed: u32) -> GradientStops {
    let a = ColorU8::rgb(seed as u8, (seed >> 8) as u8, (seed >> 16) as u8);
    let b = ColorU8::rgb((seed >> 4) as u8, (seed >> 12) as u8, 0x40);
    GradientStops::new([Stop::new(0.0, a), Stop::new(1.0, b)])
}

/// Atlas grown to `capacity` and filled to one row short of full.
///
/// Filling happens inside a single epoch so the table *grows* to the
/// target rather than evicting its way around a smaller one, then a
/// `flush` releases the epoch protection so the measured registrations
/// are free to evict.
fn filled(capacity: u32) -> CpuGradientAtlas {
    let mut atlas = CpuGradientAtlas::new(capacity);
    let mut seed = 0u32;
    while atlas.capacity() < capacity || atlas.probe.bakes() < capacity - 1 {
        seed += 1;
        atlas.register_stops(&gradient_for(seed), Interp::Oklab);
        assert!(seed < capacity * 4, "fill made no progress");
    }
    assert_eq!(atlas.capacity(), capacity);
    atlas.flush();
    atlas
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("gradient_atlas/register");
    group.sample_size(50);
    group.warm_up_time(Duration::from_millis(200));
    group.measurement_time(Duration::from_secs(2));
    group.throughput(Throughput::Elements(1));

    for capacity in CAPACITIES {
        // Steady state: the most recently filled `WORKING_SET` rows are
        // resident whatever the capacity, so only the table around them
        // differs between arms.
        let mut atlas = filled(capacity);
        let resident: Vec<GradientStops> = (capacity - WORKING_SET..capacity)
            .map(gradient_for)
            .collect();
        let before = atlas.probe.bakes();
        for stops in &resident {
            black_box(atlas.register_stops(stops, Interp::Oklab));
        }
        assert_eq!(
            atlas.probe.bakes(),
            before,
            "hit/{capacity} fixture re-baked: the working set is not resident",
        );

        let mut i = 0usize;
        group.bench_with_input(BenchmarkId::new("hit", capacity), &capacity, |b, _| {
            b.iter(|| {
                let stops = &resident[i % resident.len()];
                i = i.wrapping_add(1);
                black_box(atlas.register_stops(stops, Interp::Oklab))
            });
        });

        // Churn: a gradient never registered before, every iteration.
        // Missing is then independent of the table size, so the arms
        // differ only in how much table the miss has to work against.
        let mut atlas = filled(capacity);
        let (hits, bakes) = (atlas.probe.hits(), atlas.probe.bakes());
        for k in 0..16 {
            atlas.flush();
            black_box(atlas.register_stops(&gradient_for(CHURN_BASE + k), Interp::Oklab));
        }
        assert_eq!(
            atlas.probe.hits(),
            hits,
            "miss/{capacity} fixture hit the index: the churn seeds overlap the fill",
        );
        assert_eq!(
            atlas.probe.bakes() - bakes,
            16,
            "miss/{capacity} fixture must bake exactly once per registration",
        );

        let growths = atlas.probe.growths();
        let mut seed = CHURN_BASE + 16;
        group.bench_with_input(BenchmarkId::new("miss", capacity), &capacity, |b, _| {
            b.iter(|| {
                // `flush` is the epoch boundary; without it the rows this
                // arm just claimed stay eviction-exempt and the atlas
                // grows instead of churning.
                atlas.flush();
                seed = seed.wrapping_add(1);
                black_box(atlas.register_stops(&gradient_for(seed), Interp::Oklab))
            });
        });
        assert_eq!(
            atlas.probe.growths(),
            growths,
            "miss/{capacity} grew the atlas: it measured a ratchet, not churn",
        );
        eprintln!(
            "[gradient_atlas] capacity={capacity} rows={} evictions={} fallbacks={}",
            atlas.capacity(),
            atlas.probe.evictions(),
            atlas.probe.fallbacks(),
        );
    }
    group.finish();
}
