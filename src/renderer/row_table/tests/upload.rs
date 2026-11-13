//! The dirty runs a flush hands the GPU, and how often a steady frame rebakes.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::common::counters::CounterSet;
use crate::renderer::row_table::tests::support::{assert_real_row, distinct_grad, fresh_row};
use crate::renderer::row_table::*;
use std::collections::HashSet;

/// An idle table flushes once for the magenta init (one 2048-byte row), then stays clean.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn freshly_constructed_atlas_flushes_magenta_once() {
    let mut table = RowTable::default();
    {
        let first = table.flush().expect("first flush carries magenta init");
        assert_eq!(first.runs(), [0..1]);
        assert_eq!(first.bytes(0..1).len(), size_of::<LutRowTexels>());
    }
    assert!(table.flush().is_none());
}

/// The flush covers exactly the touched rows: one row, one run per scattered row, or `None` when
/// clean.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn flush_runs_cover_exactly_the_dirty_rows() {
    let mut table = RowTable::default();
    let _ = table.flush(); // drain the magenta init row
    let ra = table.register(&distinct_grad(10).ramp);
    {
        let f = table.flush().expect("one baked row must flush");
        assert_eq!(f.runs(), [ra.0..ra.0 + 1]);
        assert_eq!(f.bytes(ra.0..ra.0 + 1).len(), size_of::<LutRowTexels>());
    }
    let rb = table.register(&distinct_grad(20).ramp);
    let rc = table.register(&distinct_grad(30).ramp);
    assert_eq!((rb.0, rc.0), (2, 3), "claims walk ascending");
    {
        let f = table.flush().expect("two baked rows must flush");
        assert_eq!(f.runs(), [2..4], "neighbors are one run");
    }
    assert!(table.flush().is_none());
}

/// Rows re-dirtied around clean ones: a gap up to `MERGE_GAP_ROWS` rides along in one run, a
/// wider one splits it. `rows_uploaded` counts what went up, gaps included. With no CPU mirror,
/// the flush bakes every row of a run from its ramp, the clean one between included: the run's
/// bytes are the three ramps' bakes, as a mirror held them.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn a_small_gap_rides_in_the_run_and_a_wide_one_splits_it() {
    let mut table = RowTable::default();
    let _ = table.flush(); // drain the magenta init row

    let a = table.register(&distinct_grad(10).ramp);
    let b = table.register(&distinct_grad(20).ramp);
    let c = table.register(&distinct_grad(30).ramp);
    assert_eq!((a.0, b.0, c.0), (1, 2, 3), "claims walk ascending from 1");
    let _ = table.flush();
    let before = table.counters.counts();

    // Dirty the outer two directly through the one path that marks rows.
    table.mark_row_dirty(1);
    table.mark_row_dirty(3);
    let f = table.flush().expect("two dirtied rows must flush");
    assert_eq!(f.runs(), [1..4], "the one clean row between rides along");
    let mut baked = Vec::new();
    for i in [10, 20, 30] {
        let mut row = fresh_row();
        bake::row(&distinct_grad(i).ramp, &mut row);
        baked.push(row);
    }
    assert_eq!(f.bytes(1..4), bytemuck::cast_slice::<_, u8>(&baked));

    let delta = table.counters.counts() - before;
    assert_eq!(delta.bakes, 0, "no row was claimed again, only uploaded");
    assert_eq!(delta.rows_uploaded, 3);

    // Rows 4 to 10 held too, as a flush bakes held rows alone.
    for i in 0..7 {
        table.register(&distinct_grad(100 + i).ramp);
    }
    let _ = table.flush();
    // 1, then 1 + MERGE_GAP_ROWS + 1 is a gap of exactly MERGE_GAP_ROWS; one row further is not.
    let edge = 2 + MERGE_GAP_ROWS;
    table.mark_row_dirty(1);
    table.mark_row_dirty(edge);
    table.mark_row_dirty(edge + MERGE_GAP_ROWS + 2);
    let f = table.flush().expect("three dirtied rows must flush");
    assert_eq!(
        f.runs(),
        [
            1..edge + 1,
            edge + MERGE_GAP_ROWS + 2..edge + MERGE_GAP_ROWS + 3
        ]
    );
}

/// A frame redrawing unchanged gradients bakes nothing, evicts nothing and holds the table size.
#[test]
fn steady_state_frames_never_rebake() {
    const GRADIENTS: u32 = 64;
    const FRAMES: u32 = 10;

    let mut table = RowTable::default();
    let content: Vec<_> = (0..GRADIENTS).map(distinct_grad).collect();
    let rows: Vec<LutRow> = content
        .iter()
        .map(|g| table.register(&g.clone().ramp))
        .collect();
    let after_warmup = table.counters.counts().bakes;
    assert_eq!(after_warmup, GRADIENTS);

    for _ in 0..FRAMES {
        table.flush();
        for (g, &row) in content.iter().zip(&rows) {
            assert_eq!(
                table.register(&g.clone().ramp),
                row,
                "steady-state frame moved a gradient off its row",
            );
        }
    }

    let counts = table.counters.counts();
    assert_eq!(
        counts.bakes, after_warmup,
        "a steady-state frame must not bake",
    );
    assert_eq!(counts.evictions, 0);
    assert_eq!(counts.growths, 0);
    assert_eq!(counts.hits, GRADIENTS * FRAMES);
    assert_eq!(
        counts.registrations,
        GRADIENTS * (FRAMES + 1),
        "warm-up misses plus every frame's hits",
    );
    assert_eq!(table.capacity(), INITIAL_ROWS);
}

/// Churn across display frames, a ramp a frame, evicts past the soft rows and never grows: the
/// clock takes the row claimed longest ago, idle past the grace. Cycling twice the table's size,
/// every registration misses.
#[test]
fn churn_across_frames_evicts_without_growing() {
    let working_set = (INITIAL_ROWS * 2) as usize;
    let mut table = RowTable::default();
    let content: Vec<_> = (0..working_set).map(|i| distinct_grad(i as u32)).collect();

    let mut frame = 0;
    for round in 0..4 {
        for g in &content {
            frame += 1;
            table.advance_to(frame);
            let row = table.register(&g.clone().ramp);
            assert_real_row(&table, row);
        }
        assert_eq!(
            table.capacity(),
            INITIAL_ROWS,
            "round {round} grew the table instead of evicting",
        );
    }

    let registrations = (working_set * 4) as u32;
    let counts = table.counters.counts();
    assert_eq!(counts.registrations, registrations);
    assert_eq!(counts.growths, 0);
    // The first INITIAL_ROWS - 1 take never-claimed rows and the rest evict.
    assert_eq!(counts.hits, 0, "cyclic churn cannot hit");
    assert_eq!(counts.bakes, registrations);
    assert_eq!(counts.evictions, registrations - (INITIAL_ROWS - 1));
    assert_eq!(table.index_len(), (INITIAL_ROWS - 1) as usize);
}

/// A miss bakes exactly one row, and a hit none.
#[test]
fn every_miss_bakes_exactly_one_row() {
    let mut table = RowTable::default();
    let content: Vec<_> = (0..40).map(|i| distinct_grad(i as u32)).collect();
    let sequence: Vec<usize> = (0..40).chain(0..40).chain([3, 3, 17, 39, 0]).collect();

    let mut expected_bakes = 0u32;
    let mut seen = HashSet::new();
    for &i in &sequence {
        if seen.insert(i) {
            expected_bakes += 1;
        }
        table.register(&content[i].clone().ramp);
    }

    let counts = table.counters.counts();
    assert_eq!(counts.bakes, expected_bakes);
    assert_eq!(counts.bakes, 40, "each distinct gradient baked once");
    assert_eq!(
        counts.hits,
        sequence.len() as u32 - expected_bakes,
        "every non-first occurrence must resolve from the index",
    );
    assert_eq!(counts.evictions, 0, "40 gradients fit in 255 rows");
}
