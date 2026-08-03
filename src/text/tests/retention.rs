use super::*;

/// The body every retention case shapes, so a key derived in one place
/// resolves in another.
const BODY: &str = "hello world";

/// The shape `fill_distinct_widths` inserts at index `i`. A named
/// function rather than an inline literal: a case that wants to hit one
/// of those keys again asks for the same index instead of re-deriving
/// the arithmetic, so the two cannot drift apart.
fn distinct_width_shape(i: u32) -> TestShape {
    // Distinct width ⇒ distinct cache key ⇒ a fresh insert.
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
        .weight(FontWeight::Bold)
        .halign(HAlign::Center);
    let mut wrap = CosmicMeasure::with_bundled_fonts();
    let original = wrap.measure(text, wrap_params);
    let original_glyphs = glyph_positions(&wrap, original.key);
    wrap.drop_all_buffers();
    assert!(wrap.shaped_run(original.key).is_none());
    wrap.ensure_buffer(TextShapeRequest {
        text,
        key: original.key,
    });
    let restored = wrap.measure(text, wrap_params);
    assert_eq!(restored.size, original.size);
    assert_eq!(restored.intrinsic_min, original.intrinsic_min);
    assert_eq!(glyph_positions(&wrap, restored.key), original_glyphs);

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let mut truncated = CosmicMeasure::with_bundled_fonts();
        let params = wrap_params.width(84.003);
        let cut = truncate(&mut truncated, text, params, fit);
        let (original, unbounded) = (cut.fitted, cut.unbounded);
        let original_glyphs = glyph_positions(&truncated, original.key);
        truncated.drop_all_buffers();
        assert!(truncated.shaped_run(original.key).is_none(), "fit: {fit:?}");
        assert!(
            truncated.shaped_run(unbounded.key).is_none(),
            "fit: {fit:?}",
        );

        truncated.ensure_buffer(TextShapeRequest {
            text,
            key: original.key,
        });
        assert!(
            truncated.shaped_run(unbounded.key).is_some(),
            "truncation restoration must rebuild its unbounded probe for {fit:?}",
        );
        let restored = truncated.measure_with_fit(text, params, fit, unbounded.key);
        assert_eq!(restored.size, original.size, "fit: {fit:?}");
        assert_eq!(
            restored.intrinsic_min, original.intrinsic_min,
            "fit: {fit:?}",
        );
        assert_eq!(
            glyph_positions(&truncated, restored.key),
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
        .weight(FontWeight::Bold)
        .halign(HAlign::Right);
    let mut recycled = CosmicMeasure::with_bundled_fonts();
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

    let mut fresh = CosmicMeasure::with_bundled_fonts();
    let expected = fresh.measure(text, narrow);
    assert_eq!(actual.size, expected.size);
    assert_eq!(actual.intrinsic_min, expected.intrinsic_min);
    assert_eq!(
        glyph_positions(&recycled, actual.key),
        glyph_positions(&fresh, expected.key),
    );
}

#[test]
fn recycle_pool_retention_is_bounded() {
    let mut c = CosmicMeasure::with_bundled_fonts();
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

/// Shared fixture for the retention tests: `n` distinct cache keys, one
/// per width, all inserted in the current frame.
fn fill_distinct_widths(c: &mut CosmicMeasure, n: u32) -> Vec<TextShapeKey> {
    (0..n)
        .map(|i| c.measure(BODY, distinct_width_shape(i)).key)
        .collect()
}

fn idle_frames(c: &mut CosmicMeasure, n: u64) {
    for _ in 0..n {
        c.tick_frame();
    }
}

/// Retention is by age, not capacity: an untouched entry lives exactly
/// `PROBATION_KEEP_FRAMES` frames past its last touch, and no number of
/// *other* insertions can shorten that.
#[test]
fn probationary_entries_age_out_on_schedule_regardless_of_cache_size() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let keys = fill_distinct_widths(&mut c, 10);
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    // Inserted during frame 0, so the first four sweeps see a cutoff of
    // 0 (saturated) and keep them; the fifth is the first whose cutoff, 1,
    // is past their stamp.
    idle_frames(&mut c, cosmic::PROBATION_KEEP_FRAMES);
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

    // Capacity plays no part: a hundred times as many entries age out on
    // exactly the same schedule.
    let mut big = CosmicMeasure::with_bundled_fonts();
    fill_distinct_widths(&mut big, 1000);
    assert_eq!(big.cache_len(), 1000);
    idle_frames(&mut big, cosmic::PROBATION_KEEP_FRAMES);
    assert_eq!(
        big.cache_len(),
        1000,
        "a large working set is not evicted for being large",
    );
    idle_frames(&mut big, 1);
    assert_eq!(big.cache_len(), 0);
}

/// A lookup promotes an entry out of probation and onto the long window.
/// This is the whole scan-resistance mechanism: one-shot drag widths die
/// young, entries something actually came back for do not.
#[test]
fn a_lookup_promotes_an_entry_to_the_protected_window() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let keys = fill_distinct_widths(&mut c, 4);

    // An encoder ensure is a lookup like any other.
    c.ensure_buffer(TextShapeRequest {
        text: BODY,
        key: keys[0],
    });
    // A layout-side measure of the same key is too — asked for by index
    // rather than by re-deriving index 1's width.
    let reshaped = c.measure(BODY, distinct_width_shape(1));
    assert_eq!(reshaped.key, keys[1], "same parameters, same key");

    // One frame past probation: the two untouched keys are gone, the two
    // promoted ones are still here — they have 120 frames, not 4.
    idle_frames(&mut c, cosmic::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 2);
    assert!(c.shaped_run(keys[0]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[1]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[2]).is_none(), "probationary key dropped");
    assert!(c.shaped_run(keys[3]).is_none(), "probationary key dropped");

    // And they last out the protected window, then go.
    idle_frames(
        &mut c,
        cosmic::PROTECTED_KEEP_FRAMES - cosmic::PROBATION_KEEP_FRAMES - 1,
    );
    assert_eq!(c.cache_len(), 2, "still inside the protected window");
    idle_frames(&mut c, 1);
    assert_eq!(c.cache_len(), 0, "one frame past it, dropped");
}

/// The regression the age policy exists to prevent: a live label minting
/// one new key per frame must not cost anything that scales with the size
/// of the cache it lands in, and must never evict the working set around
/// it. Under the old count budget this was a full three-pass sweep every
/// frame — 5.4% of `frame/partial_cpu`.
#[test]
fn steady_key_churn_costs_a_bounded_cache_and_spares_the_working_set() {
    let mut c = CosmicMeasure::with_bundled_fonts();

    // A working set looked up every frame: promoted on the first re-read,
    // and never a candidate afterwards.
    //
    // That access pattern is this unit's contract, not the pipeline's —
    // a real steady-state frame reaches neither the shaper nor the
    // encoder's restore, because the measure cache and the encoded-run
    // cache short-circuit first. `resize_drag_retains_only_the_probation
    // _window` and its neighbours cover what the pipeline actually
    // produces; this one pins the age policy in isolation.
    let working_set = fill_distinct_widths(&mut c, 20);
    // `ensure_buffer` is exactly what the encoder calls; asserting the
    // buffer is present first means an eviction fails here rather than
    // being papered over by the reshape `ensure_buffer` would do.
    let touch_working_set = |c: &mut CosmicMeasure, working_set: &[TextShapeKey]| {
        for key in working_set {
            assert!(
                c.shaped_run(*key).is_some(),
                "a working-set key must never be evicted",
            );
            c.ensure_buffer(TextShapeRequest {
                text: BODY,
                key: *key,
            });
        }
    };

    let mut lens = Vec::new();
    for frame in 0..60u32 {
        touch_working_set(&mut c, &working_set);
        // One never-seen-before label per frame — a clock, an FPS counter,
        // a progress percentage.
        c.measure(
            &format!("tick {frame}"),
            shape(14.0).leading(18.0).width(200.0).halign(HAlign::Left),
        );
        c.tick_frame();
        lens.push(c.cache_len());
    }

    // Steady state: the 20 protected keys, plus the counter values from
    // the last PROBATION_KEEP_FRAMES frames — the sweep advances the frame
    // first, so exactly that many stamps sit at or above the cutoff.
    let steady = 20 + cosmic::PROBATION_KEEP_FRAMES as usize;
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
