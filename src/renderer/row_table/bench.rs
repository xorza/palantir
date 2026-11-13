//! Register-path scaling benchmark for the gradient rows.
//!
//! Tests **flatness, not speed**: `register` must not get more expensive as
//! the table grows. Each arm runs at two capacities; compare the ratio
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
//! [`RowTableCounters`](super::counters::RowTableCounters) first, so
//! a fixture that stops doing what its name says fails loudly.
//!
//! - `gap_row/*`: one clean row a flush folds into a run, baked again from its ramp as the flush
//!   does with no CPU mirror, against the copy a mirror made. Per row.

use crate::bench::backends::Backend;
use crate::bench::case_group::CaseGroup;
use crate::bench::kind::{Kind, Scale};
use crate::common::counters::CounterSet;
use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::row_table::bake::{self, LUT_ROW_TEXELS, LutRowTexels};
use crate::renderer::row_table::{MERGE_GAP_ROWS, RowTable, RowTableConfig};
use criterion::{Criterion, Throughput};
use std::hint::black_box;

/// Capacities compared: the initial size and three doublings up.
const CAPACITIES: [u32; 2] = [256, 2048];

/// Resident gradients the `hit` arms cycle through; fixed across capacities.
const WORKING_SET: u32 = 128;

const CHURN_BASE: u32 = 1_000_000;

/// Distinct ramp per seed, walking the color cube so every seed is a distinct
/// bake key.
///
/// Deliberately structured, not pre-mixed: constant channels across a run are
/// what a themed palette looks like and what `ColorRamp`'s hash must
/// spread, so this also guards that hash. Built from sRGB bytes, which
/// round-trip exactly.
fn gradient_for(seed: u32) -> ColorRamp {
    let a = SrgbaU8::rgb(seed as u8, (seed >> 8) as u8, (seed >> 16) as u8);
    let b = SrgbaU8::rgb((seed >> 4) as u8, (seed >> 12) as u8, 0x40);
    ColorRamp::from([RgbaF32::from_srgba8(a), RgbaF32::from_srgba8(b)])
}

/// Table grown to `capacity` and filled to one row short of full.
///
/// Fills in one display frame so the table grows rather than evicting, flushes, then moves two
/// frames on, past the grace, so the rows are free for eviction.
fn filled(capacity: u32) -> RowTable {
    let mut table = RowTable::new(RowTableConfig::of_device(capacity));
    let mut seed = 0u32;
    while table.capacity() < capacity || table.counters.counts().bakes < capacity - 1 {
        seed += 1;
        table.register(&gradient_for(seed));
        assert!(seed < capacity * 4, "fill made no progress");
    }
    assert_eq!(table.capacity(), capacity);
    table.flush();
    table.advance_to(2);
    table
}

/// `atomic/gradient_atlas/cpu/register/<hit|miss>/<capacity>`: one registration against a
/// resident working set, and one never-seen gradient that evicts and bakes. Per registration.
pub(crate) fn atomic(c: &mut Criterion) {
    let mut group = Kind::Atomic.group(c, "gradient_atlas", Scale::Sample);
    group.throughput(Throughput::Elements(1));
    for capacity in CAPACITIES {
        group.bench_function(
            Backend::Cpu.case(&format!("register/hit/{capacity}")),
            |b| {
                // Steady state: the most recent `WORKING_SET` rows are resident at any capacity.
                let mut table = filled(capacity);
                let resident: Vec<ColorRamp> = (capacity - WORKING_SET..capacity)
                    .map(gradient_for)
                    .collect();
                let before = table.counters.counts().bakes;
                for ramp in &resident {
                    black_box(table.register(ramp));
                }
                assert_eq!(
                    table.counters.counts().bakes,
                    before,
                    "hit/{capacity} fixture re-baked: the working set is not resident",
                );
                let mut i = 0usize;
                b.iter(|| {
                    let ramp = &resident[i % resident.len()];
                    i = i.wrapping_add(1);
                    black_box(table.register(ramp))
                });
            },
        );

        group.bench_function(
            Backend::Cpu.case(&format!("register/miss/{capacity}")),
            |b| {
                // Churn: a never-registered gradient every iteration, so a miss is independent of
                // table size.
                let mut table = filled(capacity);
                let (hits, bakes) = (table.counters.counts().hits, table.counters.counts().bakes);
                for k in 0..16 {
                    table.flush();
                    table.advance_to(table.frame() + 2);
                    black_box(table.register(&gradient_for(CHURN_BASE + k)));
                }
                assert_eq!(
                    table.counters.counts().hits,
                    hits,
                    "miss/{capacity} fixture hit the index: the churn seeds overlap the fill",
                );
                assert_eq!(
                    table.counters.counts().bakes - bakes,
                    16,
                    "miss/{capacity} fixture must bake exactly once per registration",
                );
                let growths = table.counters.counts().growths;
                let mut seed = CHURN_BASE + 16;
                b.iter(|| {
                    // A submit's flush bakes the row the last miss claimed; two frames on, past
                    // the grace, as otherwise the claimed rows stay exempt and the table grows
                    // instead of churning.
                    table.flush();
                    table.advance_to(table.frame() + 2);
                    seed = seed.wrapping_add(1);
                    black_box(table.register(&gradient_for(seed)))
                });
                assert_eq!(
                    table.counters.counts().growths,
                    growths,
                    "miss/{capacity} grew the table: it measured a ratchet, not churn",
                );
            },
        );
    }
    gap_row(&mut group);
    group.finish();
}

/// Where a flush finds the bytes of a clean row it folds into a run.
#[derive(Clone, Copy, Debug)]
enum GapSource {
    /// The CPU mirror, which holds every row baked.
    Mirror,
    /// A bake of the row's ramp.
    Bake,
}

/// `atomic/gradient_atlas/cpu/gap_row/<mirror|bake>/<oklab|linear>`: one clean row of a
/// [`MERGE_GAP_ROWS`] gap staged, copied from the mirror or baked again from its ramp in that
/// interpolation. Each case first checks that both give the same bytes.
fn gap_row(group: &mut CaseGroup<'_>) {
    for (interpolation, space) in [
        (Interpolation::Oklab, "oklab"),
        (Interpolation::Linear, "linear"),
    ] {
        for (source, name) in [(GapSource::Mirror, "mirror"), (GapSource::Bake, "bake")] {
            group.bench_function(Backend::Cpu.case(&format!("gap_row/{name}/{space}")), |b| {
                let ramps: Vec<ColorRamp> = (1..=MERGE_GAP_ROWS)
                    .map(|seed| gradient_for(seed).with_interpolation(interpolation))
                    .collect();
                let mut mirror: Vec<LutRowTexels> =
                    vec![[RgbaF16::TRANSPARENT; LUT_ROW_TEXELS]; ramps.len()];
                for (ramp, row) in ramps.iter().zip(&mut mirror) {
                    bake::row(ramp, row);
                }
                let mut staged = [RgbaF16::TRANSPARENT; LUT_ROW_TEXELS];
                for (ramp, row) in ramps.iter().zip(&mirror) {
                    bake::row(ramp, &mut staged);
                    assert_eq!(
                        &staged, row,
                        "{space}: a bake again gives the mirror's bytes"
                    );
                }
                let mut i = 0usize;
                b.iter(|| {
                    let at = i % ramps.len();
                    i = i.wrapping_add(1);
                    match source {
                        GapSource::Mirror => staged.copy_from_slice(&mirror[at]),
                        GapSource::Bake => bake::row(&ramps[at], &mut staged),
                    }
                    black_box(&staged);
                });
            });
        }
    }
}
