use super::*;

#[test]
fn identity_cache_is_keyed_by_actual_shaping_inputs() {
    let mut text = TextSystem::mono();
    let wid = WidgetId::from_hash("a");
    let run_slot = slot(wid);
    let compact = shape(16.0);
    let r1 = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    let calls = text.shaper.measure_calls();
    assert_eq!(r1.size, Size::new(16.0, 16.0));

    let same = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    assert_eq!(same.size, r1.size);
    assert_eq!(same.key, r1.key);
    assert_eq!(same.intrinsic_min, r1.intrinsic_min);
    assert_eq!(
        text.shaper.measure_calls(),
        calls,
        "identical shaping inputs must reuse the row",
    );

    let quantized_same = text.shape_run(
        run_slot,
        "hi",
        compact.font_size(16.006).leading(16.006),
        TextWrap::SingleLine,
    );
    assert_eq!(quantized_same.key, same.key);
    assert_eq!(quantized_same.size, same.size);
    assert_eq!(quantized_same.intrinsic_min, same.intrinsic_min);
    assert_eq!(
        text.shaper.measure_calls(),
        calls,
        "raw values in the same 1/64 px bucket must reuse the canonical row",
    );

    let r2 = text.shape_run(run_slot, "hi", compact.leading(24.0), TextWrap::SingleLine);
    assert_eq!(r2.size, Size::new(16.0, 24.0));
    assert_eq!(
        text.shaper.measure_calls(),
        calls + 1,
        "metric changes must refresh the row",
    );

    let different_text = text.shape_run(run_slot, "hello", compact, TextWrap::SingleLine);
    assert_eq!(different_text.size, Size::new(40.0, 16.0));
    assert_eq!(
        text.shaper.measure_calls(),
        calls + 2,
        "text changes must refresh the row",
    );
}

#[test]
fn identity_cache_refreshes_stale_unbounded_and_bounded_results() {
    let mut text = TextSystem::mono();
    let wid = WidgetId::from_hash("a");
    let params = shape(16.0);

    let old = text.shape_run(slot(wid), "hi", params, TextWrap::SingleLine);
    assert_eq!(old.size, Size::new(16.0, 16.0));
    assert_eq!(
        text.shape_run(slot(wid), "hi", params.width(32.0), TextWrap::Wrap)
            .size,
        Size::new(16.0, 16.0),
    );

    let current = text.shape_run(slot(wid), "abcdefgh", params, TextWrap::SingleLine);
    assert_eq!(current.size, Size::new(64.0, 16.0));
    // Eight 8 px glyphs at 32 px fit four per line: 32 px × two 16 px lines.
    assert_eq!(
        text.shape_run(slot(wid), "abcdefgh", params.width(32.0), TextWrap::Wrap)
            .size,
        Size::new(32.0, 32.0),
    );
}

/// A reuse row outlives the frames it is not used in, and goes only with
/// its widget.
///
/// It used to go after one unused frame. That lost the wrap slot — the
/// only record of which bounded key the row last answered — and with it
/// the `supersede` that demotes the key when the width next moves. Since
/// the layout measure cache short-circuits whole subtrees, a steadily
/// redrawing run never touches its row at all, so the slot was being
/// discarded constantly and every stop-start of a drag leaked a buffer
/// onto the long window.
#[test]
fn reuse_rows_outlive_unused_frames_and_go_with_their_widget() {
    let mut text = TextSystem::mono();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    let params = shape(16.0);

    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot_at(a, 1), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot(b), "yo", params, TextWrap::SingleLine);
    frame_end(&mut text);
    assert_eq!(text.entry_count(), 3, "rows used this frame all survive");

    // Second frame touches only `a`'s first row. The untouched two stay:
    // being unused for a frame is what a measure-cache hit looks like, not
    // evidence the run is gone.
    let clock = text.shaper.frame();
    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    frame_end(&mut text);
    assert_eq!(text.entry_count(), 3, "an unused frame drops nothing");
    assert!(text.has_entry(a, 1), "untouched sibling row survives");
    assert!(text.has_entry(b, 0), "untouched row of another widget too");
    // An empty `removed` skips the retain — the sweep would be a
    // whole-table walk to discover nothing changed — but must *not* skip
    // the clock, which every text cache in the crate ages against and
    // which the glyph atlas needs to advance before anything is
    // evictable. Pinned here because the two live one line apart.
    assert_eq!(
        text.shaper.frame(),
        clock + 1,
        "the empty-removed guard must not swallow the frame tick",
    );

    // A removed widget's rows go even when hot, in the same retain pass
    // that drops cold ones.
    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot(b), "yo", params, TextWrap::SingleLine);
    text.end_full_record(&FxHashSet::from_iter([a]));
    assert_eq!(text.entry_count(), 1);
    assert!(
        !text.has_entry(a, 0),
        "removed widget's row goes regardless of its hot bit",
    );
    assert!(text.has_entry(b, 0), "unrelated hot row remains");
}

/// A run driven through `TextSystem` the way a frame drives it: the
/// intrinsic pass takes the root, the measure pass resolves a width.
/// Returns the bounded key the renderer would replay.
fn drive(text: &mut TextSystem, slot: TextRunSlot, body: &str, width: Option<f32>) -> TextShapeKey {
    let base = ui_shape(14.0).halign(HAlign::Left);
    let shape = match width {
        Some(w) => base.width(w),
        None => base,
    };
    text.shape_run(slot, body, shape, TextWrap::Wrap).key
}

/// [`drive`] plus the render half: the encoder's restore on an
/// encoded-cache miss, which is the only thing that promotes a buffer
/// onto the protected window.
///
/// A test that models a *visible* run needs both halves. Layout alone
/// only ever inserts, so a layout-only fixture leaves every buffer on
/// the probation window and would report a bounded cache whether
/// supersession works or not.
fn drive_visible(
    text: &mut TextSystem,
    slot: TextRunSlot,
    body: &str,
    width: Option<f32>,
) -> TextShapeKey {
    let key = drive(text, slot, body, width);
    text.shaper
        .render_ensure(TextShapeRequest::for_key(body, key).unwrap());
    key
}

/// End a frame with nothing removed — the steady case.
fn frame_end(text: &mut TextSystem) {
    text.end_full_record(&FxHashSet::default());
}

/// Idle frames, for aging a buffer toward one of the retention windows.
fn idle(text: &mut TextSystem, frames: u64) {
    for _ in 0..frames {
        frame_end(text);
    }
}

/// A resize drag is the population the probation window exists for, and
/// the one it could not reach before `TextSystem` reported supersession:
/// every frame commits a new whole-pixel width, so every frame mints a
/// bounded key that nothing can ask for again.
///
/// Two things are asserted together because either alone is misleading.
/// The cache must stay bounded by the *probation* window rather than the
/// protected one — 60 frames of 8 runs would otherwise retain every one
/// of the 480 buffers, since a rendered run is looked up on the frame it
/// is inserted and would be promoted there. And the shaping must stay
/// proportional to the drag: one bounded reshape per run per frame is
/// the irreducible cost of the width genuinely changing, but the
/// *unbounded* root must be shaped exactly once per run for the whole
/// drag, because a width drag leaves the unbounded key untouched.
#[test]
fn resize_drag_retains_only_the_probation_window() {
    const RUNS: u32 = 8;
    const FRAMES: u32 = 60;

    let mut text = TextSystem::cosmic();
    let slots: Vec<TextRunSlot> = (0..RUNS)
        .map(|i| slot(WidgetId::from_hash(("drag", i))))
        .collect();

    // Distinct body per run: `TextShapeKey` carries no widget identity,
    // so eight runs of identical text would share one key and the drag
    // would mint one buffer a frame instead of eight.
    let bodies: Vec<String> = (0..RUNS).map(|i| format!("row {i} of the list")).collect();

    let before = text.shaper.cache_counts();
    for frame in 0..FRAMES {
        // Whole-pixel steps, so every frame quantizes to a fresh key.
        let width = 120.0 + frame as f32 * 3.0;
        for (s, body) in slots.iter().zip(&bodies) {
            drive_visible(&mut text, *s, body, Some(width));
        }
        frame_end(&mut text);
    }
    let counts = text.shaper.cache_counts() - before;

    // `TextWrap::Wrap` always binds, so a fresh run costs an unbounded
    // root plus a bounded resolve. Afterwards the reuse row answers the
    // root and only the width moves: one bounded reshape per run per
    // later frame, and the root is shaped exactly once for the whole
    // drag.
    assert_eq!(counts.shapes, RUNS * 2 + RUNS * (FRAMES - 1));
    // Every frame but the first supersedes each run's previous width.
    assert_eq!(counts.supersedes, RUNS * (FRAMES - 1));

    // Residency: the live bounded key per run, the buffers still inside
    // their shortened window, and the unbounded root per run. The
    // protected window would have held all 480 — two orders of magnitude
    // above this ceiling, so the bound is what tells the two policies
    // apart, and one assertion states it.
    let resident = text.shaper.cosmic_cache_len() as u32;
    let ceiling = RUNS * (cosmic::PROBATION_KEEP_FRAMES as u32 + 2) + RUNS;
    assert!(
        resident <= ceiling,
        "drag retained {resident} buffers, over the {ceiling} the \
         probation window allows — supersession is not reaching them",
    );
}

/// The counterweight: a run that leaves the tree is *not* superseded.
/// Scrolling a row out of view and back within the window must reuse its
/// buffer, which is exactly what the long window is for — so the fix
/// must not shorten it. Told apart from a drag by which signal fires:
/// the slot vanishes rather than moving to a new key.
#[test]
fn scrolled_away_run_keeps_the_protected_window() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("scrolled row");
    let key = drive_visible(&mut text, slot(wid), "row content", Some(200.0));
    frame_end(&mut text);

    // Out of view: the widget stops being recorded, so its reuse row is
    // dropped. Nothing supersedes the key — it may well come back.
    text.end_full_record(&FxHashSet::from_iter([wid]));
    idle(&mut text, cosmic::PROBATION_KEEP_FRAMES + 2);
    assert!(
        text.shaper.has_cosmic_buffer(key),
        "a scrolled-away run must keep the protected window",
    );

    // Back in view inside the window. The bounded buffer — the one the
    // renderer replays — is still resident, so only the unbounded root
    // is reshaped: a wrapped run's root buffer is never promoted (the
    // encoder replays the bounded key), and nothing misses it, because
    // the reuse row caches the root *value* rather than its buffer.
    let before = text.shaper.cache_counts();
    let again = drive(&mut text, slot(wid), "row content", Some(200.0));
    assert_eq!(again, key);
    assert_eq!(
        (text.shaper.cache_counts() - before).shapes,
        1,
        "the bounded buffer must survive the scroll — only the root reshapes",
    );

    // What that saved, stated as a contrast: past the protected window
    // the buffer is genuinely gone and has to be rebuilt.
    idle(&mut text, cosmic::PROTECTED_KEEP_FRAMES + 1);
    assert!(
        !text.shaper.has_cosmic_buffer(key),
        "premise: the window lapsed"
    );
    let before = text.shaper.cache_counts();
    assert_eq!(
        drive_visible(&mut text, slot(wid), "row content", Some(200.0)),
        key
    );
    assert_eq!(
        (text.shaper.cache_counts() - before).shapes,
        1,
        "a cold return rebuilds the buffer the renderer replays — and only \
         that: the reuse row still holds both measurements, so layout asks \
         for nothing",
    );
}

/// Demotion, not eviction — and that distinction is load-bearing.
/// A label alternating between two widths, or a drag that reverses back
/// through a width it just left, returns inside the probation window and
/// must still hit. Evicting on supersede would turn every reversal into
/// a reshape.
#[test]
fn superseded_key_still_hits_inside_the_probation_window() {
    let mut text = TextSystem::cosmic();
    let s = slot(WidgetId::from_hash("oscillating"));

    let narrow = drive_visible(&mut text, s, "alternating label", Some(140.0));
    frame_end(&mut text);
    // Supersedes `narrow`.
    let wide = drive_visible(&mut text, s, "alternating label", Some(260.0));
    frame_end(&mut text);
    assert_ne!(narrow, wide);

    // Back to the first width, still inside the shortened window.
    let before = text.shaper.cache_counts();
    let returned = drive(&mut text, s, "alternating label", Some(140.0));
    let counts = text.shaper.cache_counts() - before;
    assert_eq!(returned, narrow);
    assert_eq!(
        counts.shapes, 0,
        "a superseded key inside its window must be demoted, not evicted",
    );
    assert_eq!(
        counts.hits, 1,
        "one run at one width is one lookup, and it hit"
    );
}

/// Steady state must be untouched by any of this: a frame redrawing the
/// same runs at the same widths supersedes nothing and shapes nothing.
/// The reuse rows absorb it before the shaper is dispatched at all.
#[test]
fn steady_state_frames_neither_shape_nor_supersede() {
    let mut text = TextSystem::cosmic();
    let slots: Vec<TextRunSlot> = (0..4)
        .map(|i| slot(WidgetId::from_hash(("steady", i))))
        .collect();

    for s in &slots {
        drive(&mut text, *s, "unchanging label", Some(180.0));
    }
    frame_end(&mut text);

    let before = text.shaper.cache_counts();
    for _ in 0..20 {
        for s in &slots {
            drive(&mut text, *s, "unchanging label", Some(180.0));
        }
        frame_end(&mut text);
    }
    let counts = text.shaper.cache_counts() - before;
    assert_eq!(counts.shapes, 0, "steady state reshaped");
    assert_eq!(counts.supersedes, 0, "steady state superseded a live key");
    assert_eq!(counts.expiries, 0, "steady state expired a live buffer");
}

/// Typing changes the run itself, so both the unbounded row key and the
/// bounded resolve hanging off it die together — the case a width drag
/// does not cover, since a drag leaves the unbounded key alone.
#[test]
fn typing_supersedes_both_the_root_and_its_bounded_resolve() {
    let mut text = TextSystem::cosmic();
    let s = slot(WidgetId::from_hash("editor"));

    drive_visible(&mut text, s, "hell", Some(200.0));
    frame_end(&mut text);

    let before = text.shaper.cache_counts();
    drive_visible(&mut text, s, "hello", Some(200.0));
    let counts = text.shaper.cache_counts() - before;
    assert_eq!(
        counts.supersedes, 2,
        "a changed run must retire its root *and* its bounded resolve",
    );

    // And the retired pair ages out on the short window, not the long one.
    idle(&mut text, cosmic::PROBATION_KEEP_FRAMES + 2);
    let live = drive(&mut text, s, "hello", Some(200.0));
    assert!(
        text.shaper.has_cosmic_buffer(live),
        "the live run's own buffer must be resident after its reshape",
    );
    assert!(
        text.shaper.cosmic_cache_len() <= 2,
        "stale keystroke buffers outlived the probation window: {} resident",
        text.shaper.cosmic_cache_len(),
    );
}

/// Known cost, pinned so it stays known: two slots can hold the same key
/// — a grid of repeated cell text — and supersession is per-slot, so one
/// slot moving on demotes a buffer the other still uses. The worst case
/// is one reshape, never a wrong result, which is why this is accepted
/// rather than refcounted (a per-run map probe every frame to save an
/// occasional reshape is the wrong trade).
#[test]
fn shared_key_demotes_early_and_costs_at_most_one_reshape() {
    let mut text = TextSystem::cosmic();
    let (a, b) = (
        slot(WidgetId::from_hash("cell a")),
        slot(WidgetId::from_hash("cell b")),
    );

    let shared = drive_visible(&mut text, a, "—", Some(60.0));
    let same = drive_visible(&mut text, b, "—", Some(60.0));
    assert_eq!(shared, same, "identical runs must share one key");
    frame_end(&mut text);

    // Only slot `a` moves on; `b` still displays the shared key.
    drive_visible(&mut text, a, "12.5", Some(60.0));
    idle(&mut text, cosmic::PROBATION_KEEP_FRAMES + 2);
    assert!(
        !text.shaper.has_cosmic_buffer(shared),
        "premise: the shared buffer is demoted by a's move",
    );

    // The cost is bounded at one reshape — `b` recovers on its next ask,
    // and only for the buffer: its reuse row kept both measurements.
    let before = text.shaper.cache_counts();
    let recovered = drive_visible(&mut text, b, "—", Some(60.0));
    assert_eq!(recovered, shared);
    assert_eq!(
        (text.shaper.cache_counts() - before).shapes,
        1,
        "recovery costs one reshape — no more",
    );
}
