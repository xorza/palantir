//! The dirty span a flush hands the GPU, and how often a steady frame rebakes.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use crate::common::counters::CounterSet;
use crate::renderer::gradient_atlas::tests::support::{assert_real_row, distinct_grad};
use crate::renderer::gradient_atlas::*;
use std::collections::HashSet;

/// An idle atlas flushes once for the magenta init (one 2048-byte row), then stays clean.
#[test]
fn freshly_constructed_atlas_flushes_magenta_once() {
    let mut atlas = CpuGradientAtlas::default();
    {
        let first = atlas.flush().expect("first flush carries magenta init");
        assert_eq!(first.first_row, 0);
        assert_eq!(first.bytes.len(), size_of::<LutRowTexels>());
    }
    assert!(atlas.flush().is_none());
}

/// The flush range covers exactly the touched rows: one row, the `min..=max`
/// span for scattered rows, or `None` when clean.
#[test]
fn flush_range_covers_min_to_max_dirty_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row
    let ra = atlas.register(&distinct_grad(10).ramp);
    {
        let f = atlas.flush().expect("one baked row must flush");
        assert_eq!(f.first_row, ra.0);
        assert_eq!(f.bytes.len(), size_of::<LutRowTexels>());
    }
    let rb = atlas.register(&distinct_grad(20).ramp);
    let rc = atlas.register(&distinct_grad(30).ramp);
    let (min, max) = (rb.0.min(rc.0), rb.0.max(rc.0));
    {
        let f = atlas.flush().expect("two baked rows must flush");
        assert_eq!(f.first_row, min);
        assert_eq!(
            f.bytes.len(),
            (max - min + 1) as usize * size_of::<LutRowTexels>(),
        );
    }
    assert!(atlas.flush().is_none());
}

/// Two rows re-baked around a resident one upload three; `rows_uploaded`
/// against `bakes` is the only place that shows, as the tracker is a `(min, max)` pair.
#[test]
fn rows_uploaded_counts_the_whole_span_not_the_rows_that_changed() {
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
    let f = atlas.flush().expect("two dirtied rows must flush");
    assert_eq!(f.first_row, 1);
    assert_eq!(f.bytes.len(), 3 * size_of::<LutRowTexels>());

    let delta = atlas.counters.counts() - before;
    assert_eq!(delta.bakes, 0, "nothing was re-baked, only re-uploaded");
    assert_eq!(
        delta.rows_uploaded, 3,
        "row 2 rode along because it sits between the two that changed",
    );
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
