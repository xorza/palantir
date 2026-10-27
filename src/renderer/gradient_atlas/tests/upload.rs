//! The dirty runs a flush hands the GPU, and how often a steady frame rebakes.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::common::counters::CounterSet;
use crate::renderer::gradient_atlas::tests::support::{assert_real_row, distinct_grad};
use crate::renderer::gradient_atlas::*;
use std::collections::HashSet;

/// An idle atlas flushes once for the magenta init (one 2048-byte row), then stays clean.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn freshly_constructed_atlas_flushes_magenta_once() {
    let mut atlas = CpuGradientAtlas::default();
    {
        let first = atlas.flush().expect("first flush carries magenta init");
        assert_eq!(first.runs, [0..1]);
        assert_eq!(first.len_bytes(), size_of::<LutRowTexels>());
    }
    assert!(atlas.flush().is_none());
}

/// The flush covers exactly the touched rows: one row, one run per scattered row, or `None` when
/// clean.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn flush_runs_cover_exactly_the_dirty_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row
    let ra = atlas.register(&distinct_grad(10).ramp);
    {
        let f = atlas.flush().expect("one baked row must flush");
        assert_eq!(f.runs, [ra.0..ra.0 + 1]);
        assert_eq!(f.bytes(ra.0..ra.0 + 1).len(), size_of::<LutRowTexels>());
    }
    let rb = atlas.register(&distinct_grad(20).ramp);
    let rc = atlas.register(&distinct_grad(30).ramp);
    assert_eq!((rb.0, rc.0), (2, 3), "claims walk ascending");
    {
        let f = atlas.flush().expect("two baked rows must flush");
        assert_eq!(f.runs, [2..4], "neighbors are one run");
    }
    assert!(atlas.flush().is_none());
}

/// Two rows re-dirtied around a resident one upload two, in two runs: the clean row between them
/// stays on the GPU as it is, while `MERGE_GAP_ROWS` holds no gap worth a merge.
#[test]
fn rows_uploaded_counts_the_rows_that_changed_not_the_span() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row

    let a = atlas.register(&distinct_grad(10).ramp);
    let b = atlas.register(&distinct_grad(20).ramp);
    let c = atlas.register(&distinct_grad(30).ramp);
    assert_eq!((a.0, b.0, c.0), (1, 2, 3), "claims walk ascending from 1");
    let _ = atlas.flush();
    let before = atlas.counters.counts();

    // Dirty the outer two directly through the one path that marks rows.
    atlas.mark_row_dirty(1);
    atlas.mark_row_dirty(3);
    assert_eq!(MERGE_GAP_ROWS, 0, "premise: a one-row gap is not merged");
    let f = atlas.flush().expect("two dirtied rows must flush");
    assert_eq!(f.runs, [1..2, 3..4]);
    assert_eq!(f.len_bytes(), 2 * size_of::<LutRowTexels>());

    let delta = atlas.counters.counts() - before;
    assert_eq!(delta.bakes, 0, "nothing was re-baked, only re-uploaded");
    assert_eq!(delta.rows_uploaded, 2, "row 2 stays as it is");
}

/// A frame redrawing unchanged gradients bakes nothing, evicts nothing and holds the atlas size.
#[test]
fn steady_state_frames_never_rebake() {
    const GRADIENTS: u32 = 64;
    const FRAMES: u32 = 10;

    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..GRADIENTS).map(distinct_grad).collect();
    let rows: Vec<LutRow> = content
        .iter()
        .map(|g| atlas.register(&g.clone().ramp))
        .collect();
    let after_warmup = atlas.counters.counts().bakes;
    assert_eq!(after_warmup, GRADIENTS);

    for _ in 0..FRAMES {
        atlas.flush();
        for (g, &row) in content.iter().zip(&rows) {
            assert_eq!(
                atlas.register(&g.clone().ramp),
                row,
                "steady-state frame moved a gradient off its row",
            );
        }
    }

    let counts = atlas.counters.counts();
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
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);
}

/// Churn across epochs evicts but never grows, as growth is one-way. Cycling
/// twice the table's size is LRU's worst case, so every registration misses.
#[test]
fn cross_epoch_churn_evicts_without_growing() {
    let working_set = (INITIAL_ATLAS_ROWS * 2) as usize;
    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..working_set).map(|i| distinct_grad(i as u32)).collect();

    for round in 0..4 {
        for g in &content {
            atlas.flush();
            let row = atlas.register(&g.clone().ramp);
            assert_real_row(&atlas, row);
        }
        assert_eq!(
            atlas.capacity(),
            INITIAL_ATLAS_ROWS,
            "round {round} grew the atlas instead of evicting",
        );
    }

    let registrations = (working_set * 4) as u32;
    let counts = atlas.counters.counts();
    assert_eq!(counts.registrations, registrations);
    assert_eq!(counts.growths, 0);
    // The first INITIAL_ATLAS_ROWS - 1 take never-claimed rows and the rest evict.
    assert_eq!(counts.hits, 0, "cyclic churn cannot hit");
    assert_eq!(counts.bakes, registrations);
    assert_eq!(counts.evictions, registrations - (INITIAL_ATLAS_ROWS - 1));
    assert_eq!(atlas.index_len(), (INITIAL_ATLAS_ROWS - 1) as usize);
}

/// A miss bakes exactly one row; a growth once baked a resident gradient twice.
#[test]
fn every_miss_bakes_exactly_one_row() {
    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..40).map(|i| distinct_grad(i as u32)).collect();
    let sequence: Vec<usize> = (0..40).chain(0..40).chain([3, 3, 17, 39, 0]).collect();

    let mut expected_bakes = 0u32;
    let mut seen = HashSet::new();
    for &i in &sequence {
        if seen.insert(i) {
            expected_bakes += 1;
        }
        atlas.register(&content[i].clone().ramp);
    }

    let counts = atlas.counters.counts();
    assert_eq!(counts.bakes, expected_bakes);
    assert_eq!(counts.bakes, 40, "each distinct gradient baked once");
    assert_eq!(
        counts.hits,
        sequence.len() as u32 - expected_bakes,
        "every non-first occurrence must resolve from the index",
    );
    assert_eq!(counts.evictions, 0, "40 gradients fit in 255 rows");
}
