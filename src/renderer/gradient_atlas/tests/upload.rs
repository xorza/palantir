//! The dirty span a flush hands the GPU, and how often a steady frame
//! rebakes.

use crate::renderer::gradient_atlas::tests::support::{
    assert_real_row, distinct_grad, register_for,
};
use crate::renderer::gradient_atlas::*;
use std::collections::HashSet;

/// `flush` returns `Some(...)` once after a register, then `None`
/// until the next register. Idle-frame upload is zero bytes.
#[test]
fn flush_returns_bytes_once_then_none() {
    let mut atlas = CpuGradientAtlas::default();
    register_for(&mut atlas, distinct_grad(0.3));
    assert!(atlas.flush().is_some(), "dirty atlas must yield bytes");
    assert!(
        atlas.flush().is_none(),
        "second flush without register is none"
    );
}

/// Idle atlas (no registrations beyond magenta init) hits the
/// `Some` branch once for the magenta upload — covering exactly the
/// one dirty row (row 0, 2048 bytes), not the whole 512 KB atlas —
/// then stays clean.
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

/// The flush range covers exactly the rows touched since the last
/// flush: one baked row → that single 2048-byte row at its own
/// index; two scattered rows → the contiguous min..=max span
/// (`(max - min + 1) × 2048` bytes starting at min); nothing dirty
/// → `None`.
#[test]
fn flush_range_covers_min_to_max_dirty_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row
    // Single row: range is exactly [row, row].
    let ra = register_for(&mut atlas, distinct_grad(0.1));
    {
        let f = atlas.flush().expect("one baked row must flush");
        assert_eq!(f.first_row, ra.0);
        assert_eq!(f.bytes.len(), size_of::<LutRowTexels>());
    }
    // Two scattered rows: range spans min..=max, whole rows.
    let rb = register_for(&mut atlas, distinct_grad(0.2));
    let rc = register_for(&mut atlas, distinct_grad(0.3));
    let (min, max) = (rb.0.min(rc.0), rb.0.max(rc.0));
    {
        let f = atlas.flush().expect("two baked rows must flush");
        assert_eq!(f.first_row, min);
        assert_eq!(
            f.bytes.len(),
            (max - min + 1) as usize * size_of::<LutRowTexels>(),
        );
    }
    // Clean atlas: nothing to upload.
    assert!(atlas.flush().is_none());
}

/// What the span above *costs*, reported rather than left to be
/// inferred from a byte length nothing reads back.
///
/// Two rows re-baked with a resident row between them upload three, and
/// `rows_uploaded` against `bakes` is the only place that shows. The
/// gap is what a scattered dirty set pays: the tracker is a `(min, max)`
/// pair, so it can say "rows 1 through 3" and never "rows 1 and 3".
#[test]
fn rows_uploaded_counts_the_whole_span_not_the_rows_that_changed() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush(); // drain the magenta init row

    // Three consecutive rows, then a flush that clears the dirty range.
    let a = register_for(&mut atlas, distinct_grad(0.1));
    let b = register_for(&mut atlas, distinct_grad(0.2));
    let c = register_for(&mut atlas, distinct_grad(0.3));
    assert_eq!((a.0, b.0, c.0), (1, 2, 3), "claims walk ascending from 1");
    let _ = atlas.flush();
    let before = atlas.counters.counts();

    // Re-bake only the outer two, by evicting them: register two fresh
    // gradients after filling the table would be a bigger fixture, so
    // dirty them directly through the one path that marks rows.
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

/// The headline steady-state property: a frame redrawing unchanged
/// gradients bakes nothing. Every registration resolves from the index,
/// nothing is evicted, and the atlas holds its size.
///
/// This is what the cache is *for*, and before the probe existed there
/// was no way to tell it from a cache that re-baked every row and
/// happened to return the same ids.
#[test]
fn steady_state_frames_never_rebake() {
    const GRADIENTS: u32 = 64;
    const FRAMES: u32 = 10;

    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..GRADIENTS)
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();
    let rows: Vec<LutRow> = content
        .iter()
        .map(|g| register_for(&mut atlas, g.clone()))
        .collect();
    let after_warmup = atlas.counters.counts().bakes;
    assert_eq!(after_warmup, GRADIENTS);

    for _ in 0..FRAMES {
        atlas.flush();
        for (g, &row) in content.iter().zip(&rows) {
            assert_eq!(
                register_for(&mut atlas, g.clone()),
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

/// Churn across epochs evicts; it must never grow.
///
/// This is the ratchet guard. Growth is one-way — the atlas has no
/// shrink path — so a workload that grows the table when it should have
/// evicted permanently enlarges every structure the register path
/// touches. A gradient animated per frame produces exactly this:
/// a working set far larger than the table, none of it reused.
///
/// Cycling a set twice the table's size is LRU's worst case by
/// construction, so every registration here is a miss. That is the
/// point — it is the shape most likely to trip a grow-instead-of-evict
/// bug, and each round crosses an epoch boundary the way a real frame
/// does.
#[test]
fn cross_epoch_churn_evicts_without_growing() {
    let working_set = (INITIAL_ATLAS_ROWS * 2) as usize;
    let mut atlas = CpuGradientAtlas::default();
    let content: Vec<_> = (0..working_set)
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();

    for round in 0..4 {
        for g in &content {
            atlas.flush();
            let row = register_for(&mut atlas, g.clone());
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
    // Cyclic access over 2x the table never reuses a resident row, so
    // every registration misses; the first INITIAL_ATLAS_ROWS - 1 take
    // never-claimed rows and the rest evict.
    assert_eq!(counts.hits, 0, "cyclic churn cannot hit");
    assert_eq!(counts.bakes, registrations);
    assert_eq!(counts.evictions, registrations - (INITIAL_ATLAS_ROWS - 1),);
    assert_eq!(atlas.index_len(), (INITIAL_ATLAS_ROWS - 1) as usize);
}

/// A miss bakes exactly one row — never two.
///
/// The old table could bake a resident gradient a second time after a
/// growth moved its probe base, which showed up only as a quietly
/// wasted row. Pinning bakes against misses makes any repeat bake a
/// failure rather than a slow leak.
#[test]
fn every_miss_bakes_exactly_one_row() {
    let mut atlas = CpuGradientAtlas::default();
    // Mixed traffic: fresh content, immediate repeats, and repeats of
    // content registered several steps back.
    let content: Vec<_> = (0..40).map(|i| distinct_grad(i as f32 * 0.01)).collect();
    let sequence: Vec<usize> = (0..40).chain(0..40).chain([3, 3, 17, 39, 0]).collect();

    let mut expected_bakes = 0u32;
    let mut seen = HashSet::new();
    for &i in &sequence {
        if seen.insert(i) {
            expected_bakes += 1;
        }
        register_for(&mut atlas, content[i].clone());
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
