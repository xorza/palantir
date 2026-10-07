use super::*;

/// The body every retention case shapes.
const BODY: &str = "hello world";

/// The shape `fill_distinct_widths` inserts at index `i`.
fn distinct_width_shape(i: u32) -> TestShape {
    shape(14.0)
        .leading(18.0)
        .width(40.0 + i as f32 * 5.0)
        .halign(HAlign::Left)
}

#[test]
fn ensure_buffer_exactly_restores_wrap_and_truncation() {
    let text = "restore this shaped buffer after eviction";
    let wrap_params = shape(15.003)
        .leading(18.003)
        .width(96.003)
        .weight(FontWeight::BOLD)
        .halign(HAlign::Center);
    let mut wrap = CosmicMeasure::default();
    let original = wrap.measure(text, wrap_params);
    let original_glyphs = glyph_positions(&wrap, original.buffer_key());
    wrap.drop_all_buffers();
    assert!(wrap.shaped_run(original.buffer_key()).is_none());
    wrap.ensure_buffer(TextShapeRequest::for_key(text, original.buffer_key()).unwrap());
    let restored = wrap.measure(text, wrap_params);
    assert_eq!(restored.size, original.size);
    assert_eq!(restored.intrinsic_min, original.intrinsic_min);
    assert_eq!(
        glyph_positions(&wrap, restored.buffer_key()),
        original_glyphs
    );

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let mut truncated = CosmicMeasure::default();
        let params = wrap_params.width(84.003);
        let cut = truncate(&mut truncated, text, params, fit);
        let (original, unbounded) = (cut.fitted, cut.unbounded);
        let original_glyphs = glyph_positions(&truncated, original.buffer_key());
        truncated.drop_all_buffers();
        assert!(
            truncated.shaped_run(original.buffer_key()).is_none(),
            "fit: {fit:?}"
        );
        assert!(
            truncated.shaped_run(unbounded.buffer_key()).is_none(),
            "fit: {fit:?}",
        );

        truncated.ensure_buffer(TextShapeRequest::for_key(text, original.buffer_key()).unwrap());
        assert!(
            truncated.shaped_run(unbounded.buffer_key()).is_some(),
            "truncation restoration must rebuild its unbounded probe for {fit:?}",
        );
        let restored = truncated.measure_with_fit(text, params, fit, unbounded.buffer_key());
        assert_eq!(restored.size, original.size, "fit: {fit:?}");
        assert_eq!(
            restored.intrinsic_min, original.intrinsic_min,
            "fit: {fit:?}",
        );
        assert_eq!(
            glyph_positions(&truncated, restored.buffer_key()),
            original_glyphs,
            "fit: {fit:?}",
        );
    }
}

#[test]
fn recycled_buffer_matches_fresh_shape_at_new_width() {
    let text = "recycled cosmic buffers must reshape exactly across a new wrapping width";
    let base = shape(15.0)
        .leading(18.0)
        .width(180.0)
        .weight(FontWeight::BOLD)
        .halign(HAlign::Right);
    let mut recycled = CosmicMeasure::default();
    recycled.measure(text, base);
    recycled.drop_all_buffers();
    assert_eq!(recycled.recycle_pool_stats().len, 1);

    let narrow = base.width(72.0);
    let actual = recycled.measure(text, narrow);
    assert_eq!(
        recycled.recycle_pool_stats().len,
        0,
        "the new miss must consume the evicted buffer",
    );

    let mut fresh = CosmicMeasure::default();
    let expected = fresh.measure(text, narrow);
    assert_eq!(actual.size, expected.size);
    assert_eq!(actual.intrinsic_min, expected.intrinsic_min);
    assert_eq!(
        glyph_positions(&recycled, actual.buffer_key()),
        glyph_positions(&fresh, expected.buffer_key()),
    );
}

#[test]
fn recycle_pool_retention_is_bounded() {
    let mut c = CosmicMeasure::default();
    let pool = c.recycle_pool_stats();
    assert!(pool.capacity >= pool.limit);

    for round in 0..2 {
        for i in 0..pool.limit + 16 {
            let width = 40.0 + (round * (pool.limit + 16) + i) as f32;
            c.measure(
                "bounded recycle pool",
                shape(14.0).leading(18.0).width(width).halign(HAlign::Left),
            );
        }
        c.drop_all_buffers();
        let after = c.recycle_pool_stats();
        assert_eq!(after.len, pool.limit);
        assert_eq!(after.capacity, pool.capacity);
        assert_eq!(after.limit, pool.limit);
    }
}

/// `n` distinct cache keys, one per width, all inserted this frame.
fn fill_distinct_widths(c: &mut CosmicMeasure, n: u32) -> Vec<TextShapeKey> {
    (0..n)
        .map(|i| c.measure(BODY, distinct_width_shape(i)).buffer_key())
        .collect()
}

fn idle_frames(c: &mut CosmicMeasure, n: u64) {
    for _ in 0..n {
        c.tick_frame();
    }
}

/// Retention is by age, not capacity: an untouched entry lives exactly
/// `PROBATION_KEEP_FRAMES` frames past its last touch.
#[test]
fn probationary_entries_age_out_on_schedule_regardless_of_cache_size() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 10);
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    // Inserted in frame 0: the first four sweeps have a saturated cutoff of 0; the
    // fifth is the first whose cutoff passes their stamp.
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES);
    assert_eq!(
        c.cache_len(),
        10,
        "an entry survives its whole probation window",
    );
    idle_frames(&mut c, 1);
    assert_eq!(c.cache_len(), 0, "one frame past the window, all dropped");
    for key in &keys {
        assert!(c.shaped_run(*key).is_none());
    }

    let mut big = CosmicMeasure::default();
    fill_distinct_widths(&mut big, 1000);
    assert_eq!(big.cache_len(), 1000);
    idle_frames(&mut big, shaped_buffer_cache::PROBATION_KEEP_FRAMES);
    assert_eq!(
        big.cache_len(),
        1000,
        "a large working set is not evicted for being large",
    );
    idle_frames(&mut big, 1);
    assert_eq!(big.cache_len(), 0);
}

/// A lookup promotes an entry out of probation onto the long window.
#[test]
fn a_lookup_promotes_an_entry_to_the_protected_window() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 4);

    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    let reshaped = c.measure(BODY, distinct_width_shape(1));
    assert_eq!(reshaped.buffer_key(), keys[1], "same parameters, same key");

    // Untouched keys are gone; promoted ones have 120 frames, not 4.
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 2);
    assert!(c.shaped_run(keys[0]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[1]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[2]).is_none(), "probationary key dropped");
    assert!(c.shaped_run(keys[3]).is_none(), "probationary key dropped");

    // The protected window is `RENDERED_RUN_KEEP_SPREAD_MASK` frames wide, each key
    // at its own point: this pins floor and ceiling.
    idle_frames(
        &mut c,
        RENDERED_RUN_KEEP_FRAMES - shaped_buffer_cache::PROBATION_KEEP_FRAMES - 1,
    );
    assert_eq!(c.cache_len(), 2, "inside the window every key is promised");
    idle_frames(&mut c, RENDERED_RUN_KEEP_SPREAD_MASK + 1);
    assert_eq!(c.cache_len(), 0, "past the widest of them, both dropped");
}

/// A live label minting one new key per frame must neither cost anything scaling
/// with cache size nor evict the working set.
#[test]
fn steady_key_churn_costs_a_bounded_cache_and_spares_the_working_set() {
    let mut c = CosmicMeasure::default();

    // Looked up every frame: promoted on first re-read, never a candidate after.
    let working_set = fill_distinct_widths(&mut c, 20);
    // Asserting presence first makes an eviction fail here rather than be hidden by
    // `ensure_buffer`'s reshape.
    let touch_working_set = |c: &mut CosmicMeasure, working_set: &[TextShapeKey]| {
        for key in working_set {
            assert!(
                c.shaped_run(*key).is_some(),
                "a working-set key must never be evicted",
            );
            c.ensure_buffer(TextShapeRequest::for_key(BODY, *key).unwrap());
        }
    };

    let mut lens = Vec::new();
    for frame in 0..60u32 {
        touch_working_set(&mut c, &working_set);
        c.measure(
            &format!("tick {frame}"),
            shape(14.0).leading(18.0).width(200.0).halign(HAlign::Left),
        );
        c.tick_frame();
        lens.push(c.cache_len());
    }

    // The 20 protected keys plus the counter values of the last
    // PROBATION_KEEP_FRAMES frames.
    let steady = 20 + shaped_buffer_cache::PROBATION_KEEP_FRAMES as usize;
    assert_eq!(
        lens[10..],
        vec![steady; 50][..],
        "churn must settle at a fixed size, not grow and not thrash",
    );
    for key in &working_set {
        assert!(
            c.shaped_run(*key).is_some(),
            "60 frames of churn must not have touched the working set",
        );
    }
}

/// A resize drag demotes and promotes runs every frame. A supplanted ticket that
/// re-filed itself instead of dying would grow the ticket count each cycle, so
/// sweep cost would scale with uptime.
#[test]
fn demote_and_promote_churn_keeps_the_ticket_count_flat() {
    const RUNS: usize = 8;
    const WIDTHS: usize = 4;
    const FRAMES: usize = 400;
    const SETTLED_BY: usize = 100;

    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, (RUNS * WIDTHS) as u32);
    let idx = |run: usize, width: usize| run * WIDTHS + width;

    let mut pending_at = Vec::new();
    for frame in 0..FRAMES {
        let width = frame % WIDTHS;
        let previous = (frame + WIDTHS - 1) % WIDTHS;
        for run in 0..RUNS {
            c.measure(BODY, distinct_width_shape(idx(run, width) as u32));
            if frame > 0 {
                c.supersede(keys[idx(run, previous)]);
            }
        }
        c.tick_frame();
        if frame % WIDTHS == 0 {
            pending_at.push((frame, c.pending_tickets()));
        }
    }

    // Nothing is evicted, so any growth is ticket surplus.
    assert_eq!(c.cache_len(), RUNS * WIDTHS, "the working set is intact");

    // A `supersede` ticket is the live one and fires PROBATION_KEEP_FRAMES + 1
    // frames later; the supplanted one dies on its next firing. Outstanding is one
    // live ticket per resident entry plus demotes in flight.
    let ceiling = RUNS * WIDTHS + RUNS * (shaped_buffer_cache::PROBATION_KEEP_FRAMES as usize + 2);
    let (worst_frame, worst) = *pending_at.iter().max_by_key(|&&(_, n)| n).unwrap();
    assert!(
        worst <= ceiling,
        "frame {worst_frame} held {worst} tickets, over the {ceiling} \
         a churning entry can justify",
    );

    let settled = pending_at
        .iter()
        .find(|&&(frame, _)| frame == SETTLED_BY)
        .expect("the sample cadence divides SETTLED_BY");
    let last = pending_at.last().unwrap();
    assert_eq!(
        settled.1, last.1,
        "frame {} and frame {} must hold the same ticket count — it \
         tracks churn, not how long the drag has run",
        settled.0, last.0,
    );
}

/// A demote must take effect while a longer-lived ticket is outstanding; retiring
/// the supplanted ticket instead would keep a dead buffer resident.
#[test]
fn a_demote_still_evicts_on_time_with_an_older_ticket_outstanding() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 1);

    // Promote it, then let its insert-time ticket re-file out to the protected
    // deadline.
    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 1, "promoted, so it outlives probation");

    c.supersede(keys[0]);
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES);
    assert_eq!(c.cache_len(), 1, "still inside the probation window");
    idle_frames(&mut c, 1);
    assert_eq!(
        c.cache_len(),
        0,
        "a demoted entry dies on the probation window, not the protected one",
    );
}

/// A demoted entry the drag returns to is promoted again; the supplanted ticket
/// must not evict it when it fires.
#[test]
fn a_supplanted_ticket_does_not_evict_an_entry_promoted_since() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 1);

    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);

    c.supersede(keys[0]);
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES);
    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());

    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    assert_eq!(c.cache_len(), 1, "the promotion outranks the stale ticket");

    idle_frames(
        &mut c,
        RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK,
    );
    assert_eq!(c.cache_len(), 0, "left alone, it still ages out");
}

/// Retention is spread across frames past the window's floor, so a burst promoted
/// together does not expire on one frame (a page switch promotes hundreds of runs,
/// and dropping them together would free cosmic allocations in one frame).
#[test]
fn a_promoted_burst_expires_across_frames_rather_than_on_one() {
    const RUNS: u32 = 64;

    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, RUNS);
    for &key in &keys {
        c.ensure_buffer(TextShapeRequest::for_key(BODY, key).unwrap());
    }
    assert_eq!(c.cache_len() as u32, RUNS);

    idle_frames(&mut c, RENDERED_RUN_KEEP_FRAMES);
    assert_eq!(
        c.cache_len() as u32,
        RUNS,
        "no entry may die before the window's floor",
    );

    // Frame `floor + 1 + k` takes exactly the keys whose offset is `k`.
    let mut live = c.cache_len();
    let mut dropped = Vec::new();
    for _ in 0..=RENDERED_RUN_KEEP_SPREAD_MASK {
        idle_frames(&mut c, 1);
        dropped.push(live - c.cache_len());
        live = c.cache_len();
    }
    assert_eq!(live, 0, "the whole burst is gone by the ceiling");

    let expected: Vec<usize> = (0..=RENDERED_RUN_KEEP_SPREAD_MASK)
        .map(|offset| {
            keys.iter()
                .filter(|key| key.keep_spread() == offset)
                .count()
        })
        .collect();
    assert_eq!(dropped, expected, "each key drops on its own offset");
    assert!(
        dropped.iter().filter(|&&n| n > 0).count() > 1,
        "premise: {RUNS} runs must not all share one offset — got {dropped:?}",
    );
}
