//! The dirty runs a flush hands the GPU, and how often a steady frame rebakes.

use crate::common::counters::CounterSet;
use crate::renderer::row_table::tests::support::{
    assert_real_row, baked, distinct_grad, fill_rows, ramp, runs,
};
use crate::renderer::row_table::*;
use std::collections::HashSet;

/// An idle table flushes once for the magenta init, one row, then stays clean.
#[test]
fn a_new_table_flushes_magenta_once() {
    let mut table = RowTable::default();
    assert_eq!(runs(&mut table), [(0, 1)]);
    assert_eq!(runs(&mut table), []);
}

/// The flush covers exactly the touched rows: one row, neighbors as one run, or nothing when
/// clean.
#[test]
fn flush_runs_cover_exactly_the_dirty_rows() {
    let mut table = RowTable::default();
    runs(&mut table);
    let a = table.register(&ramp(10));
    assert_eq!(runs(&mut table), [(a.0, a.0 + 1)]);
    let b = table.register(&ramp(20));
    let c = table.register(&ramp(30));
    assert_eq!((b.0, c.0), (2, 3), "claims walk ascending");
    assert_eq!(runs(&mut table), [(2, 4)], "neighbors are one run");
    assert_eq!(runs(&mut table), []);
}

/// Rows dirtied around clean ones: a gap up to `MERGE_GAP_ROWS` rides along in one run, a wider
/// one splits it. `rows_uploaded` counts what went up, gaps included. With no CPU mirror, the flush
/// bakes every row of a run from its ramp, the clean one between included: the run's rows are the
/// three ramps' bakes, as a mirror held them.
#[test]
fn a_small_gap_rides_in_the_run_and_a_wide_one_splits_it() {
    let mut table = RowTable::default();
    for i in 0..10 {
        table.register(&ramp(10 * i + 10));
    }
    runs(&mut table);
    let before = table.counters.counts();

    table.dirty.mark(1);
    table.dirty.mark(3);
    let flush = table.flush();
    assert_eq!(
        flush.runs().len(),
        1,
        "the one clean row between rides along"
    );
    assert_eq!(flush.runs()[0], 1..4);
    for (row, i) in (1..4).zip([10, 20, 30]) {
        let mut texels = baked(&ramp(0));
        flush.bake(row, &mut texels);
        assert_eq!(texels, baked(&ramp(i)), "row {row}");
    }
    let delta = table.counters.counts() - before;
    assert_eq!(delta.bakes, 0, "no row was claimed again, only uploaded");
    assert_eq!(delta.rows_uploaded, 3);

    // 1, then 1 + MERGE_GAP_ROWS + 1 is a gap of exactly MERGE_GAP_ROWS; one row further is not.
    let edge = 2 + MERGE_GAP_ROWS;
    for row in [1, edge, edge + MERGE_GAP_ROWS + 2] {
        table.dirty.mark(row);
    }
    let far = edge + MERGE_GAP_ROWS + 2;
    assert_eq!(runs(&mut table), [(1, edge + 1), (far, far + 1)]);
}

/// A frame redrawing unchanged gradients bakes nothing, evicts nothing and holds the table's
/// size: 64 ramps take a table grown from 16 rows to 128, three doublings.
#[test]
fn steady_state_frames_never_rebake() {
    const GRADIENTS: u32 = 64;
    const FRAMES: u32 = 10;

    let mut table = RowTable::default();
    let rows = fill_rows(&mut table, GRADIENTS);
    let after_warmup = table.counters.counts().bakes;
    assert_eq!(after_warmup, GRADIENTS);

    for frame in 1..=FRAMES {
        table.advance_to(u64::from(frame));
        runs(&mut table);
        assert_eq!(fill_rows(&mut table, GRADIENTS), rows);
    }

    let counts = table.counters.counts();
    assert_eq!(
        counts.bakes, after_warmup,
        "a steady-state frame must not bake"
    );
    assert_eq!(counts.evictions, 0);
    assert_eq!(counts.growths, 3);
    assert_eq!(counts.hits, GRADIENTS * FRAMES);
    assert_eq!(counts.registrations, GRADIENTS * (FRAMES + 1));
    assert_eq!(table.capacity(), 128);
}

/// Churn across display frames, a ramp a frame, grows to the soft rows, four doublings from 16,
/// then evicts and never grows: the clock takes the row claimed longest ago, idle past the grace.
/// Cycling twice the soft rows, every registration misses.
#[test]
fn churn_across_frames_evicts_past_the_soft_rows() {
    let working_set = SOFT_ROWS * 2;
    let mut table = RowTable::default();
    let mut frame = 0;
    for round in 0..4 {
        for i in 0..working_set {
            frame += 1;
            table.advance_to(frame);
            let row = table.register(&ramp(i));
            assert_real_row(&table, row);
        }
        assert_eq!(table.capacity(), SOFT_ROWS, "round {round}");
    }

    let registrations = working_set * 4;
    let counts = table.counters.counts();
    assert_eq!(counts.registrations, registrations);
    assert_eq!(counts.growths, 4);
    assert_eq!(counts.hits, 0, "cyclic churn cannot hit");
    assert_eq!(counts.bakes, registrations);
    assert_eq!(counts.evictions, registrations - (SOFT_ROWS - 1));
    assert_eq!(table.index_len(), (SOFT_ROWS - 1) as usize);
}

/// A miss bakes exactly one row, and a hit none.
#[test]
fn every_miss_bakes_exactly_one_row() {
    let mut table = RowTable::default();
    let sequence: Vec<u32> = (0..40).chain(0..40).chain([3, 3, 17, 39, 0]).collect();
    let mut seen = HashSet::new();
    for &i in &sequence {
        seen.insert(i);
        table.register(&distinct_grad(i).ramp);
    }
    let counts = table.counters.counts();
    assert_eq!(counts.bakes, 40, "each distinct gradient baked once");
    assert_eq!(seen.len(), 40);
    assert_eq!(counts.hits, sequence.len() as u32 - 40);
    assert_eq!(counts.evictions, 0, "40 gradients fit under the soft rows");
}
