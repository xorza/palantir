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
        .weight(FontWeight::BOLD)
        .halign(HAlign::Center);
    let mut wrap = CosmicMeasure::default();
    let original = wrap.measure(text, wrap_params);
    let original_glyphs = glyph_positions(&wrap, original.key);
    wrap.drop_all_buffers();
    assert!(wrap.shaped_run(original.key).is_none());
    wrap.ensure_buffer(TextShapeRequest::for_key(text, original.key).unwrap());
    let restored = wrap.measure(text, wrap_params);
    assert_eq!(restored.size, original.size);
    assert_eq!(restored.intrinsic_min, original.intrinsic_min);
    assert_eq!(glyph_positions(&wrap, restored.key), original_glyphs);

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let mut truncated = CosmicMeasure::default();
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

        truncated.ensure_buffer(TextShapeRequest::for_key(text, original.key).unwrap());
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
        glyph_positions(&recycled, actual.key),
        glyph_positions(&fresh, expected.key),
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
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 10);
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    // Inserted during frame 0, so the first four sweeps see a cutoff of
    // 0 (saturated) and keep them; the fifth is the first whose cutoff, 1,
    // is past their stamp.
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

    // Capacity plays no part: a hundred times as many entries age out on
    // exactly the same schedule.
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

/// A lookup promotes an entry out of probation and onto the long window.
/// This is the whole scan-resistance mechanism: one-shot drag widths die
/// young, entries something actually came back for do not.
#[test]
fn a_lookup_promotes_an_entry_to_the_protected_window() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 4);

    // An encoder ensure is a lookup like any other.
    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    // A layout-side measure of the same key is too — asked for by index
    // rather than by re-deriving index 1's width.
    let reshaped = c.measure(BODY, distinct_width_shape(1));
    assert_eq!(reshaped.key, keys[1], "same parameters, same key");

    // One frame past probation: the two untouched keys are gone, the two
    // promoted ones are still here — they have 120 frames, not 4.
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 2);
    assert!(c.shaped_run(keys[0]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[1]).is_some(), "promoted key survives");
    assert!(c.shaped_run(keys[2]).is_none(), "probationary key dropped");
    assert!(c.shaped_run(keys[3]).is_none(), "probationary key dropped");

    // And they last out the protected window, then go. That window is a
    // range `PROTECTED_SPREAD_MASK` frames wide and each key sits at its
    // own point in it, so what is pinned here is the floor and the
    // ceiling rather than one shared edge.
    idle_frames(
        &mut c,
        RENDERED_RUN_KEEP_FRAMES - shaped_buffer_cache::PROBATION_KEEP_FRAMES - 1,
    );
    assert_eq!(c.cache_len(), 2, "inside the window every key is promised");
    idle_frames(&mut c, RENDERED_RUN_KEEP_SPREAD_MASK + 1);
    assert_eq!(c.cache_len(), 0, "past the widest of them, both dropped");
}

/// The regression the age policy exists to prevent: a live label minting
/// one new key per frame must not cost anything that scales with the size
/// of the cache it lands in, and must never evict the working set around
/// it. Under the old count budget this was a full three-pass sweep every
/// frame — 5.4% of `frame/partial_cpu`.
#[test]
fn steady_key_churn_costs_a_bounded_cache_and_spares_the_working_set() {
    let mut c = CosmicMeasure::default();

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
            c.ensure_buffer(TextShapeRequest::for_key(BODY, *key).unwrap());
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

/// A resize drag demotes each run's previous width and promotes the one
/// it lands on, every frame, forever. Each demote files a ticket that
/// supplants the entry's outstanding one — and if the supplanted ticket
/// re-files itself instead of dying, the ticket count grows by one per
/// run per cycle for as long as the entry stays resident. The sweep then
/// costs a function of uptime rather than of churn: `frame/resizing_cpu`
/// read 374 µs before that and 707 µs after, and kept climbing with the
/// benchmark's own measurement window.
#[test]
fn demote_and_promote_churn_keeps_the_ticket_count_flat() {
    const RUNS: usize = 8;
    const WIDTHS: usize = 4;
    // Long enough that unbounded growth is unmissable: one surplus
    // ticket per run per frame would leave ~3200 outstanding by the end.
    const FRAMES: usize = 400;
    // Past the initial fill's own insert tickets, so both samples sit in
    // steady state rather than one catching the ramp.
    const SETTLED_BY: usize = 100;

    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, (RUNS * WIDTHS) as u32);
    // One spelling of the arithmetic, so the key a case supersedes and
    // the shape it measures cannot drift apart.
    let idx = |run: usize, width: usize| run * WIDTHS + width;

    // Sampled at matching phase of the rotation, so the two are directly
    // comparable rather than differing by where in the cycle they land.
    let mut pending_at = Vec::new();
    for frame in 0..FRAMES {
        let width = frame % WIDTHS;
        let previous = (frame + WIDTHS - 1) % WIDTHS;
        for run in 0..RUNS {
            // Layout asks for this frame's width: a hit, which promotes.
            c.measure(BODY, distinct_width_shape(idx(run, width) as u32));
            // ...and the width the run's reuse slot just stopped
            // answering is demoted to probation.
            if frame > 0 {
                c.supersede(keys[idx(run, previous)]);
            }
        }
        c.tick_frame();
        if frame % WIDTHS == 0 {
            pending_at.push((frame, c.pending_tickets()));
        }
    }

    // Nothing is ever evicted here — every key is re-measured one frame
    // inside its probation window — so any growth is pure ticket surplus.
    assert_eq!(c.cache_len(), RUNS * WIDTHS, "the working set is intact");

    // A ticket filed by `supersede` is the entry's live one and fires
    // PROBATION_KEEP_FRAMES + 1 frames later; the one it supplanted dies
    // on its own next firing. So what is outstanding is one live ticket
    // per resident entry plus the demotes still in flight, and never a
    // multiple of how long the drag has run.
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

/// The demote has to *take effect* while a longer-lived ticket is still
/// outstanding, which is the whole reason `supersede` files a second one.
/// Retiring the supplanted ticket instead of the live one would leave a
/// dead buffer resident for the protected window — the resize-drag bound
/// gone, in exchange for the flat ticket count above.
#[test]
fn a_demote_still_evicts_on_time_with_an_older_ticket_outstanding() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 1);

    // Promote it, then let its insert-time ticket fire and re-file far
    // out — now the outstanding ticket sits at the protected deadline.
    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);
    assert_eq!(c.cache_len(), 1, "promoted, so it outlives probation");

    // The reuse slot moves off this width.
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

/// The other side of the same edge: a demoted entry the drag comes back
/// to is promoted again, and the ticket the demote supplanted must not
/// take it with it when it fires. Dropping a live entry here would make
/// every reversal in a drag reshape from scratch.
#[test]
fn a_supplanted_ticket_does_not_evict_an_entry_promoted_since() {
    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, 1);

    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 1);

    // Demote, then come back to it one frame before it would lapse —
    // exactly what a width rotation does.
    c.supersede(keys[0]);
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES);
    c.ensure_buffer(TextShapeRequest::for_key(BODY, keys[0]).unwrap());

    // Walk past both the probation deadline the demote set and the
    // frame the supplanted ticket was filed for.
    idle_frames(&mut c, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    assert_eq!(c.cache_len(), 1, "the promotion outranks the stale ticket");

    // And it is still on a real deadline, not immortal.
    idle_frames(
        &mut c,
        RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK,
    );
    assert_eq!(c.cache_len(), 0, "left alone, it still ages out");
}

/// Retention is spread across the frames past the window's floor, so a
/// burst promoted on one frame does not come due on one frame.
///
/// A page switch promotes a few hundred runs together. Without the
/// spread every one of them drops on the same later frame, and past the
/// recycle pool each drop frees cosmic's line, shape and layout
/// allocations — one frame paying for what a whole navigation created.
/// `fill_distinct_widths` is that burst in miniature, and it is also the
/// case an offset taken from the text hash alone would miss: one body at
/// many widths is one text and many keys.
#[test]
fn a_promoted_burst_expires_across_frames_rather_than_on_one() {
    const RUNS: u32 = 64;

    let mut c = CosmicMeasure::default();
    let keys = fill_distinct_widths(&mut c, RUNS);
    for &key in &keys {
        c.ensure_buffer(TextShapeRequest::for_key(BODY, key).unwrap());
    }
    assert_eq!(c.cache_len() as u32, RUNS);

    // The floor is the part every entry is promised.
    idle_frames(&mut c, RENDERED_RUN_KEEP_FRAMES);
    assert_eq!(
        c.cache_len() as u32,
        RUNS,
        "no entry may die before the window's floor",
    );

    // Past it they go a frame's share at a time. Frame `floor + 1 + k`
    // takes exactly the keys whose offset is `k`.
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
