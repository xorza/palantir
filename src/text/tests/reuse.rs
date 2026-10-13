use super::*;

#[test]
fn identity_cache_is_keyed_by_actual_shaping_inputs() {
    let mut text = TextSystem::mono();
    let wid = WidgetId::from_hash("a");
    let run_slot = slot(wid);
    let compact = shape(16.0);
    let r1 = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    let calls = text.shaper().measure_calls();
    assert_eq!(r1.size, Size::new(16.0, 16.0));

    let same = text.shape_run(run_slot, "hi", compact, TextWrap::SingleLine);
    assert_eq!(same.size, r1.size);
    assert_eq!(same.intrinsic_min, r1.intrinsic_min);
    assert_eq!(
        text.shaper().measure_calls(),
        calls,
        "identical shaping inputs must reuse the row",
    );

    let quantized_same = text.shape_run(
        run_slot,
        "hi",
        compact.font_size(16.006).leading(16.006),
        TextWrap::SingleLine,
    );
    assert_eq!(quantized_same.size, same.size);
    assert_eq!(quantized_same.intrinsic_min, same.intrinsic_min);
    assert_eq!(
        text.shaper().measure_calls(),
        calls,
        "raw values in the same 1/64 px bucket must reuse the canonical row",
    );

    let r2 = text.shape_run(run_slot, "hi", compact.leading(24.0), TextWrap::SingleLine);
    assert_eq!(r2.size, Size::new(16.0, 24.0));
    assert_eq!(
        text.shaper().measure_calls(),
        calls + 1,
        "metric changes must refresh the row",
    );

    let different_text = text.shape_run(run_slot, "hello", compact, TextWrap::SingleLine);
    assert_eq!(different_text.size, Size::new(40.0, 16.0));
    assert_eq!(
        text.shaper().measure_calls(),
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
    assert_eq!(
        text.shape_run(slot(wid), "abcdefgh", params.width(32.0), TextWrap::Wrap)
            .size,
        Size::new(32.0, 32.0),
    );
}

/// A reuse row outlives the frames it is not used in and goes only with its widget. It used to go after one unused frame, losing the wrap slot and the `supersede` that demotes the key; a measure-cache hit never touches a steady run's row, so every drag leaked a buffer onto the long window.
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

    // Second frame touches only `a`'s first row; the rest stay (an unused frame is what a measure-cache hit looks like).
    let clock = text.shaper().frame();
    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    frame_end(&mut text);
    assert_eq!(text.entry_count(), 3, "an unused frame drops nothing");
    assert!(text.has_entry(a, 1), "untouched sibling row survives");
    assert!(text.has_entry(b, 0), "untouched row of another widget too");
    // An empty `removed` skips the retain walk but must not skip the clock, which every text cache and the glyph atlas age against.
    assert_eq!(
        text.shaper().frame(),
        clock + 1,
        "the empty-removed guard must not swallow the frame tick",
    );

    text.shape_run(slot_at(a, 0), "hi", params, TextWrap::SingleLine);
    text.shape_run(slot(b), "yo", params, TextWrap::SingleLine);
    frame_end_removing(&mut text, &WidgetIdSet::from_iter([a]));
    assert_eq!(text.entry_count(), 1);
    assert!(
        !text.has_entry(a, 0),
        "removed widget's row goes regardless of its hot bit",
    );
    assert!(text.has_entry(b, 0), "unrelated hot row remains");
}

/// The reuse fixture's runs, driven through `TextSystem` as a frame does.
trait DriveRuns {
    /// `body` at `width`, shaped as layout shapes it; returns the bounded key the renderer would replay.
    fn drive(&mut self, slot: TextRunSlot, body: &str, width: f32) -> TextShapeKey;

    /// [`Self::drive`] plus the encoder's restore on an encoded-cache miss, the only promotion onto the protected window; a layout-only fixture would report a bounded cache either way.
    fn drive_visible(&mut self, slot: TextRunSlot, body: &str, width: f32) -> TextShapeKey;
}

impl DriveRuns for TextSystem {
    fn drive(&mut self, slot: TextRunSlot, body: &str, width: f32) -> TextShapeKey {
        let shape = ui_shape(14.0).halign(HAlign::Left).width(width);
        self.shape_run(slot, body, shape, TextWrap::Wrap)
            .buffer_key()
    }

    fn drive_visible(&mut self, slot: TextRunSlot, body: &str, width: f32) -> TextShapeKey {
        let key = self.drive(slot, body, width);
        self.shaper()
            .render_ensure(TextShapeRequest::for_key(body, key).unwrap());
        key
    }
}

/// One frame boundary as production drives it: sweep rows of whatever left the tree, then tick the shared clock (the caller's job, as in the window runtime).
fn frame_end_removing(text: &mut TextSystem, removed: &WidgetIdSet) {
    text.end_frame(removed);
    text.shaper().tick_frame();
}

fn frame_end(text: &mut TextSystem) {
    frame_end_removing(text, &WidgetIdSet::default());
}

fn idle(text: &mut TextSystem, frames: u64) {
    for _ in 0..frames {
        frame_end(text);
    }
}

/// A resize drag mints a new bounded key every frame that nothing can ask for again. Asserted together: the cache stays bounded by the probation window (the protected one would retain all 480 buffers of 8 runs over 60 frames), and the unbounded root is shaped exactly once per run, one bounded reshape per run per frame being irreducible.
#[test]
fn resize_drag_retains_only_the_probation_window() {
    const RUNS: u32 = 8;
    const FRAMES: u32 = 60;

    let mut text = TextSystem::cosmic();
    let slots: Vec<TextRunSlot> = (0..RUNS)
        .map(|i| slot(WidgetId::from_hash(("drag", i))))
        .collect();

    let bodies: Vec<String> = (0..RUNS).map(|i| format!("row {i} of the list")).collect();

    let before = text.shaper().cache_counts();
    for frame in 0..FRAMES {
        let width = 120.0 + frame as f32 * 3.0;
        for (s, body) in slots.iter().zip(&bodies) {
            text.drive_visible(*s, body, width);
        }
        frame_end(&mut text);
    }
    let counts = text.shaper().cache_counts() - before;

    assert_eq!(counts.shapes, RUNS * 2 + RUNS * (FRAMES - 1));
    assert_eq!(counts.supersedes, RUNS * (FRAMES - 1));

    let resident = text.shaper().cosmic_cache_len() as u32;
    let ceiling = RUNS * (shaped_buffer_cache::PROBATION_KEEP_FRAMES as u32 + 2) + RUNS;
    assert!(
        resident <= ceiling,
        "drag retained {resident} buffers, over the {ceiling} the \
         probation window allows — supersession is not reaching them",
    );
}

/// A run that stops answering through its bounded slot demotes the buffer it named. Three ways to stop: the width moving, a truncating run whose box grows until the text fits (`WrapCommit::Unbounded`), and a policy that stopped binding. The last two once left the buffer on the protected window.
#[test]
fn a_run_that_stops_binding_demotes_the_buffer_its_bound_named() {
    const BODY: &str = "a rather long label that will not fit";
    let narrow = ui_shape(14.0).width(40.0);
    for (label, second_shape, second_wrap) in [
        (
            "the box grew past the text",
            ui_shape(14.0).width(400.0),
            TextWrap::Ellipsis,
        ),
        (
            "the policy stopped binding",
            ui_shape(14.0),
            TextWrap::SingleLine,
        ),
    ] {
        let mut text = TextSystem::cosmic();
        let s = slot(WidgetId::from_hash("row"));
        let bounded = text
            .shape_run(s, BODY, narrow, TextWrap::Ellipsis)
            .buffer_key();
        text.shaper()
            .render_ensure(TextShapeRequest::for_key(BODY, bounded).unwrap());
        assert!(
            bounded.max_width().is_some(),
            "{label}: premise — the narrow box binds a width",
        );

        let before = text.shaper().cache_counts();
        let after = text.shape_run(s, BODY, second_shape, second_wrap);
        assert!(
            after.buffer_key().max_width().is_none(),
            "{label}: premise — the second measure answers unbounded",
        );
        assert_eq!(
            (text.shaper().cache_counts() - before).supersedes,
            1,
            "{label}: the bounded buffer is demoted exactly once",
        );

        idle(&mut text, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
        assert!(
            !text.shaper().has_cosmic_buffer(bounded),
            "{label}: a demoted buffer must not outlive the probation window",
        );
    }
}

/// A widget that leaves the tree loses every row it holds, and no other widget loses any.
#[test]
fn a_removed_widget_loses_all_its_rows_and_no_others() {
    let mut text = TextSystem::cosmic();
    let gone = WidgetId::from_hash("gone");
    let kept = WidgetId::from_hash("kept");
    for i in 0..3 {
        text.drive(slot_at(gone, i), &format!("gone {i}"), 200.0);
    }
    for i in 0..2 {
        text.drive(slot_at(kept, i), &format!("kept {i}"), 200.0);
    }
    assert_eq!(text.entry_count(), 5);

    let removed: WidgetIdSet = [gone].into_iter().collect();
    frame_end_removing(&mut text, &removed);
    assert_eq!(text.entry_count(), 2, "5 rows less the removed widget's 3");
    assert!(text.has_entry(kept, 0) && text.has_entry(kept, 1));
    assert!(!text.has_entry(gone, 0));
}

/// A widget that records fewer runs than last time loses the rows above its new count, else rows stay at its peak ordinal count (a list that went from a hundred to three keeps ninety-seven). Their buffers are left alone; see [`scrolled_away_run_keeps_the_protected_window`].
#[test]
fn a_shrinking_widget_loses_the_rows_above_its_run_count() {
    let mut text = TextSystem::cosmic();
    let w = WidgetId::from_hash("list");
    let keys: Vec<TextShapeKey> = (0..4)
        .map(|i| {
            let body = format!("row {i}");
            text.drive_visible(slot_at(w, i), &body, 200.0)
        })
        .collect();
    assert_eq!(text.entry_count(), 4);

    let before = text.shaper().cache_counts();
    text.trim_rows(w, 4);
    assert_eq!(
        text.entry_count(),
        4,
        "a count that did not shrink retires nothing",
    );

    text.trim_rows(w, 1);
    assert_eq!(text.entry_count(), 1, "the three rows above the count go");
    assert!(text.has_entry(w, 0), "the run still recorded keeps its row");
    assert!(!text.has_entry(w, 3));
    assert_eq!(
        (text.shaper().cache_counts() - before).supersedes,
        0,
        "a slot that stopped being recorded is not a slot that moved",
    );

    idle(&mut text, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    for key in &keys {
        assert!(
            text.shaper().has_cosmic_buffer(*key),
            "a trimmed row must not shorten what its buffer was promised",
        );
    }
}

/// The counterweight: a run that leaves the tree is not superseded, so scrolling a row out and back within the window reuses its buffer.
#[test]
fn scrolled_away_run_keeps_the_protected_window() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("scrolled row");
    let key = text.drive_visible(slot(wid), "row content", 200.0);
    frame_end(&mut text);

    frame_end_removing(&mut text, &WidgetIdSet::from_iter([wid]));
    idle(&mut text, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    assert!(
        text.shaper().has_cosmic_buffer(key),
        "a scrolled-away run must keep the protected window",
    );

    // Back in view: the bounded buffer is still resident, so only the unbounded root reshapes (the reuse row caches its value, not a buffer).
    let before = text.shaper().cache_counts();
    let again = text.drive(slot(wid), "row content", 200.0);
    assert_eq!(again, key);
    assert_eq!(
        (text.shaper().cache_counts() - before).shapes,
        1,
        "the bounded buffer must survive the scroll — only the root reshapes",
    );

    idle(
        &mut text,
        RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK + 1,
    );
    assert!(
        !text.shaper().has_cosmic_buffer(key),
        "premise: the window lapsed"
    );
    let before = text.shaper().cache_counts();
    assert_eq!(text.drive_visible(slot(wid), "row content", 200.0), key);
    assert_eq!(
        (text.shaper().cache_counts() - before).shapes,
        1,
        "a cold return rebuilds the buffer the renderer replays — and only \
         that: the reuse row still holds both measurements, so layout asks \
         for nothing",
    );
}

/// Demotion, not eviction: a label alternating between widths, or a drag reversing, returns inside the probation window and must hit.
#[test]
fn superseded_key_still_hits_inside_the_probation_window() {
    let mut text = TextSystem::cosmic();
    let s = slot(WidgetId::from_hash("oscillating"));

    let narrow = text.drive_visible(s, "alternating label", 140.0);
    frame_end(&mut text);
    let wide = text.drive_visible(s, "alternating label", 260.0);
    frame_end(&mut text);
    assert_ne!(narrow, wide);

    let before = text.shaper().cache_counts();
    let returned = text.drive(s, "alternating label", 140.0);
    let counts = text.shaper().cache_counts() - before;
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

/// Steady state is untouched: redrawing the same runs at the same widths supersedes and shapes nothing.
#[test]
fn steady_state_frames_neither_shape_nor_supersede() {
    let mut text = TextSystem::cosmic();
    let slots: Vec<TextRunSlot> = (0..4)
        .map(|i| slot(WidgetId::from_hash(("steady", i))))
        .collect();

    for s in &slots {
        text.drive(*s, "unchanging label", 180.0);
    }
    frame_end(&mut text);

    let before = text.shaper().cache_counts();
    for _ in 0..20 {
        for s in &slots {
            text.drive(*s, "unchanging label", 180.0);
        }
        frame_end(&mut text);
    }
    let counts = text.shaper().cache_counts() - before;
    assert_eq!(counts.shapes, 0, "steady state reshaped");
    assert_eq!(counts.supersedes, 0, "steady state superseded a live key");
    assert_eq!(counts.expiries, 0, "steady state expired a live buffer");
}

/// Typing changes the run itself, so the unbounded row key and the bounded resolve die together; a width drag does not cover this.
#[test]
fn typing_supersedes_both_the_root_and_its_bounded_resolve() {
    let mut text = TextSystem::cosmic();
    let s = slot(WidgetId::from_hash("editor"));

    text.drive_visible(s, "hell", 200.0);
    frame_end(&mut text);

    let before = text.shaper().cache_counts();
    text.drive_visible(s, "hello", 200.0);
    let counts = text.shaper().cache_counts() - before;
    assert_eq!(
        counts.supersedes, 2,
        "a changed run must retire its root *and* its bounded resolve",
    );

    idle(&mut text, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    let live = text.drive(s, "hello", 200.0);
    assert!(
        text.shaper().has_cosmic_buffer(live),
        "the live run's own buffer must be resident after its reshape",
    );
    // The live bounded resolve alone: `drive_visible` promoted it, and both
    // `hell` buffers and the `hello` root aged out on probation.
    assert_eq!(
        text.shaper().cosmic_cache_len(),
        1,
        "stale keystroke buffers outlived the probation window",
    );
}

/// Known cost, pinned: two slots can hold one key (repeated cell text) and supersession is per-slot, so one moving on demotes a buffer the other uses. Worst case one reshape, accepted over refcounting.
#[test]
fn shared_key_demotes_early_and_costs_at_most_one_reshape() {
    let mut text = TextSystem::cosmic();
    let (a, b) = (
        slot(WidgetId::from_hash("cell a")),
        slot(WidgetId::from_hash("cell b")),
    );

    let shared = text.drive_visible(a, "—", 60.0);
    let same = text.drive_visible(b, "—", 60.0);
    assert_eq!(shared, same, "identical runs must share one key");
    frame_end(&mut text);

    text.drive_visible(a, "12.5", 60.0);
    idle(&mut text, shaped_buffer_cache::PROBATION_KEEP_FRAMES + 2);
    assert!(
        !text.shaper().has_cosmic_buffer(shared),
        "premise: the shared buffer is demoted by a's move",
    );

    // Bounded at one reshape: `b` recovers on its next ask.
    let before = text.shaper().cache_counts();
    let recovered = text.drive_visible(b, "—", 60.0);
    assert_eq!(recovered, shared);
    assert_eq!(
        (text.shaper().cache_counts() - before).shapes,
        1,
        "recovery costs one reshape — no more",
    );
}
