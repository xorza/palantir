//! What the harness *enforces* — the protocol rules that have no
//! signature. Cases that would pass just as well against a bare `Ui`
//! belong with the subsystem they pin, not here.

use super::*;
use crate::input::keyboard::KeyboardEvent;
use crate::layout::types::sizing::Sizing;
use crate::scene::node::Configure;
use crate::ui::frame_report::FrameProcessing;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;

const SURFACE: UVec2 = UVec2::new(200, 120);

fn target() -> WidgetId {
    WidgetId::from_hash("harness-target")
}

/// A 100×40 button at the surface origin, so a press at (50, 20) lands
/// inside it and (150, 100) does not.
fn button(ui: &mut Ui) {
    Panel::hstack().auto_id().show(ui, |ui| {
        Button::new()
            .id(target())
            .label("x")
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .show(ui);
    });
}

const INSIDE: Vec2 = Vec2::new(50.0, 20.0);
const OUTSIDE: Vec2 = Vec2::new(150.0, 100.0);

/// Counts record-closure invocations per frame — the fact the whole
/// protocol follows from.
#[derive(Default)]
struct PassCounter(u32);

impl PassCounter {
    fn take(&mut self) -> u32 {
        std::mem::replace(&mut self.0, 0)
    }
}

#[test]
fn warm_constructors_run_one_pass_and_cold_runs_two() {
    // Rule 1. `cold` leaves `prev_stamp` unseeded so frame 1 adds the
    // blackout warmup pass; every other constructor seeds it. The count
    // is the contract — `FrameProcessing` cannot report the warmup, so a
    // test that got this wrong would silently read the input-blind pass.
    let mut passes = PassCounter::default();

    let mut warm = UiHarness::new(SURFACE);
    let report = warm.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 1, "a warm frame 1 records once");
    assert_eq!(report.processing, FrameProcessing::SingleLayout);

    let mut cold = UiHarness::cold(SURFACE);
    cold.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 2, "a cold frame 1 records warmup + pass A");

    // Frame 2 is single either way — the warmup is a frame-1 event only.
    cold.frame(|ui| {
        passes.0 += 1;
        button(ui);
    });
    assert_eq!(passes.take(), 1);
}

#[test]
fn frame_value_returns_pass_a_not_the_drained_second_pass() {
    // Rules 4 and 5. A click makes the frame double-record; pass B runs
    // after `drain_per_frame_queues`, so it sees `clicked() == false`.
    // Reading the last pass — the `let mut x = …; frame(|ui| x = …)`
    // shape — silently loses the edge.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(INSIDE);

    // Instrumenting the raw `frame` closure, which runs on every pass —
    // this is what a caller writing `let mut x = …; frame(|ui| x = …)`
    // would end up reading.
    let mut per_pass = Vec::new();
    let report = harness.frame(|ui| {
        button(ui);
        per_pass.push(ui.response_for(target()).left.clicked());
    });

    assert_eq!(report.processing, FrameProcessing::DoubleLayout);
    assert_eq!(
        per_pass,
        vec![true, false],
        "the click frame records twice and only pass A sees the edge",
    );

    // Same click, through `frame_value`: the scene still records on both
    // passes, but the value comes from the one that saw the edge.
    harness.click_at(INSIDE);
    let mut passes = 0;
    let clicked = harness.frame_value(|ui| {
        button(ui);
        passes += 1;
        ui.response_for(target()).left.clicked()
    });

    assert_eq!(passes, 2, "frame_value must not skip pass B's recording");
    assert!(clicked, "…but must return pass A's value");
}

#[test]
fn response_in_sees_the_click_that_a_between_frames_read_misses() {
    // Rule 3. `frame_quiescent` is snapshotted at record-pass start, so
    // a read taken between frames reflects the *previous* frame's input.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(INSIDE);

    let stale = harness.ui.response_for(target()).left.clicked();
    assert!(!stale, "a between-frames read cannot see input fed since");

    let inside = harness.response_in(target(), button);
    assert!(inside.left.clicked(), "the same click, read inside pass A");
}

#[test]
fn a_press_outside_the_widget_does_not_click_it() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(OUTSIDE);
    assert!(!harness.response_in(target(), button).left.clicked());
}

#[test]
fn hit_at_reports_what_the_pointer_would_reach() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    assert_eq!(
        harness.hit_at(INSIDE),
        Some(target()),
        "the button senses hover at its own center",
    );
    assert_eq!(
        harness.hit_at(OUTSIDE),
        None,
        "nothing senses input on bare surface",
    );
}

#[test]
fn center_of_matches_the_arranged_rect() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    let rect = harness.rect(target()).expect("the button arranged");
    // A 100×40 button, first child of an origin-anchored hstack.
    assert_eq!(rect.size.w, 100.0);
    assert_eq!(rect.size.h, 40.0);
    assert_eq!(harness.center_of(target()), rect.center());
    assert_eq!(harness.hit_at(harness.center_of(target())), Some(target()));
}

#[test]
fn the_clock_only_reaches_input_through_a_frame() {
    // Rules 6 and 7. Time is frozen unless advanced, so two clicks at one
    // point are always a double-click; and `advance` alone does nothing —
    // `Ui::frame` is what publishes the clock to the input machine, so
    // the separating frame is load-bearing.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    harness.click_at(INSIDE);
    let first = harness.response_in(target(), button);
    assert!(first.left.clicked());
    assert!(!first.left.double_clicked(), "one click is not a double");

    harness.click_at(INSIDE);
    let second = harness.response_in(target(), button);
    assert!(
        second.left.double_clicked(),
        "with the clock frozen the second click is always a double",
    );

    // Advancing *and* framing separates the runs.
    harness.advance_past_double_click(button);
    harness.click_at(INSIDE);
    let third = harness.response_in(target(), button);
    assert!(third.left.clicked());
    assert!(
        !third.left.double_clicked(),
        "past DOUBLE_CLICK_WINDOW the run restarts",
    );

    // Advancing without a frame in between leaves the input clock where
    // it was, so this pairs with the click above instead of restarting.
    harness.advance(DOUBLE_CLICK_WINDOW * 2);
    harness.click_at(INSIDE);
    let fourth = harness.response_in(target(), button);
    assert!(
        fourth.left.double_clicked(),
        "advance without a frame does not reach the input clock",
    );
}

#[test]
fn double_click_at_is_two_clicks_inside_the_window() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.double_click_at(INSIDE);
    assert!(harness.response_in(target(), button).left.double_clicked());
}

#[test]
fn drag_to_latches_past_the_threshold_and_panics_under_it() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    harness.press_at(INSIDE);
    harness.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    assert!(
        harness.response_in(target(), button).left.drag.started(),
        "travel past DRAG_THRESHOLD latches a drag",
    );
    harness.release();

    let under = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut harness = UiHarness::new(SURFACE);
        harness.prime(2, button);
        harness.press_at(INSIDE);
        harness.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD - 1.0, 0.0));
    }));
    assert!(
        under.is_err(),
        "sub-threshold travel must panic, not quietly fail to latch",
    );

    let unpressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        UiHarness::new(SURFACE).drag_to(INSIDE);
    }));
    assert!(unpressed.is_err(), "drag_to needs a press first");
}

#[test]
fn modifiers_are_sticky_and_key_mods_restores_them() {
    // Rule 13. `ModifiersChanged` carries a snapshot that persists, so a
    // helper that sets them for one key must put them back.
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let mut harness = UiHarness::new(SURFACE);

    harness.key_mods(Key::Char('a'), ctrl);
    assert_eq!(
        harness.mods,
        Modifiers::NONE,
        "key_mods restores the previous set",
    );

    harness.set_modifiers(ctrl);
    harness.key(Key::Char('b'));
    assert_eq!(harness.mods, ctrl, "set_modifiers is the sticky path");

    harness.key_mods(Key::Char('c'), Modifiers::NONE);
    assert_eq!(harness.mods, ctrl, "…and key_mods restores back onto it");
}

#[test]
fn typed_text_and_ime_commits_take_different_paths() {
    // Rule 14. `type_text` emits the `KeyDown { Key::Char }` a real
    // window produces; `ime_commit` emits `Text`, chunked as a commit is.
    // `TextEdit` consumes both, which is why they are separate calls.
    let mut harness = UiHarness::new(SURFACE);
    // Keyboard events are dropped at *ingress* when nothing holds focus
    // and no subscriber matches — not queued and ignored, discarded. So
    // a keyboard test has to establish focus before it drives anything.
    harness.ui().request_focus(Some(target()));

    harness.type_text("hi");
    // 20 bytes — longer than one chunk, so it splits at a char boundary.
    harness.ime_commit("abcdefghijklmnopqrst");

    let events: Vec<_> = harness
        .ui
        .input
        .keyboard_events(Layer::Main)
        .iter()
        .map(|event| match event {
            KeyboardEvent::Down(press) => format!("down {:?}", press.key),
            KeyboardEvent::Text(chunk) => format!("text {:?}", chunk.as_str()),
        })
        .collect();

    assert_eq!(
        events,
        vec![
            "down Char('h')".to_string(),
            "down Char('i')".to_string(),
            "text \"abcdefghijklmno\"".to_string(),
            "text \"pqrst\"".to_string(),
        ],
        "type_text emits KeyDown per char; ime_commit emits chunked Text",
    );
}

#[test]
fn text_chunk_split_never_cuts_a_codepoint() {
    // 'é' is two bytes, so a 15-byte cap lands mid-codepoint on a naive
    // split at exactly the boundary this string was chosen to hit.
    let s = "ééééééééé";
    let chunks: Vec<_> = TextChunk::split(s).collect();
    let rejoined: String = chunks.iter().map(|c| c.as_str()).collect();

    assert_eq!(rejoined, s, "chunks rejoin to the original");
    assert!(
        chunks
            .iter()
            .all(|c| c.as_str().len() <= TextChunk::INLINE_CAP),
        "every chunk fits the inline capacity",
    );
    assert_eq!(chunks.len(), 2, "18 bytes over a 15-byte cap is two chunks");
    assert_eq!(chunks[0].as_str(), "ééééééé", "cut at 14, not 15");
    assert_eq!(TextChunk::split("").count(), 0, "empty yields nothing");
}

#[test]
fn scroll_routes_to_whatever_the_pointer_moved_over() {
    // Rule 12. Scroll carries no position; the target is the last hover.
    let scroller = WidgetId::from_hash("scroller");
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(scroller)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };

    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, build);

    harness.scroll_lines_at(INSIDE, Vec2::new(0.0, 3.0));
    let hit = harness.response_in(scroller, build);
    assert_eq!(hit.scroll.lines.y, 3.0, "the hovered widget got the delta");

    harness.scroll_lines_at(OUTSIDE, Vec2::new(0.0, 3.0));
    let missed = harness.response_in(scroller, build);
    assert_eq!(
        missed.scroll.lines.y, 0.0,
        "a scroll over bare surface reaches nobody",
    );
}

#[test]
fn resize_changes_the_surface_without_rebuilding_the_harness() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    assert_eq!(harness.display().physical, SURFACE);

    let bigger = UVec2::new(400, 300);
    harness.resize(bigger);
    harness.frame(button);

    assert_eq!(harness.display().physical, bigger);
    assert_eq!(harness.ui.display.physical, bigger);
}

#[test]
fn scale_makes_the_surface_physical_and_positions_logical() {
    // Rule 10. At dpr 2 a 200×120 physical surface is 100×60 logical,
    // and pointer positions are in the latter.
    let harness = UiHarness::new(SURFACE).scale(2.0);
    let display = harness.display();

    assert_eq!(display.physical, SURFACE);
    assert_eq!(display.scale_factor, 2.0);
    assert_eq!(display.logical_size().w, 100.0);
    assert_eq!(display.logical_size().h, 60.0);
}

#[test]
fn prime_stable_stops_when_rects_stop_moving() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime_stable(8, button);

    let rect = harness.rect(target()).expect("converged with a rect");
    assert_eq!(rect.size.w, 100.0);

    // A layout that never settles must say so rather than silently
    // returning a mid-animation frame.
    let never = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut harness = UiHarness::new(SURFACE);
        let mut width = 0.0f32;
        harness.prime_stable(4, |ui| {
            width += 10.0;
            Panel::hstack()
                .id(target())
                .size((Sizing::fixed(width), Sizing::fixed(40.0)))
                .show(ui, |_| {});
        });
    }));
    assert!(never.is_err(), "non-convergence must panic");
}

#[test]
fn collisions_surface_duplicate_explicit_ids() {
    // Two siblings under one explicit id — invisible at runtime except
    // as a magenta overlay, and invisible to a test without this.
    let mut harness = UiHarness::new(SURFACE);
    harness.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            for _ in 0..2 {
                Button::new().id(target()).label("dup").show(ui);
            }
        });
    });

    let collisions = harness.collisions();
    assert_eq!(collisions.len(), 1, "one colliding pair");
    assert_eq!(collisions[0].0, target(), "reported under the explicit id");

    let mut clean = UiHarness::new(SURFACE);
    clean.frame(button);
    clean.assert_no_collisions();
}

#[test]
fn clipboard_round_trips_through_the_harness() {
    let mut harness = UiHarness::new(SURFACE);
    assert_eq!(harness.clipboard_text(), "");
    harness.set_clipboard_text("copied");
    assert_eq!(harness.clipboard_text(), "copied");
}

#[test]
fn arena_interns_without_ever_recording() {
    let mut harness = UiHarness::arena();
    let interned = harness.ui().intern("label");
    assert_eq!(&*interned.borrow_str(), "label");
}

#[test]
fn from_resources_pairs_two_harnesses_onto_one_text_cache() {
    let shared = HostShared::new(TextShaper::new(), None);
    let mut first = UiHarness::from_resources(shared.resources.clone(), SURFACE);
    let second = UiHarness::from_resources(shared.resources.clone(), SURFACE);

    first.set_clipboard_text("shared");
    assert_eq!(
        second.clipboard_text(),
        "shared",
        "one HostShared, one clipboard",
    );
}

#[test]
fn advance_frames_rejects_a_step_that_would_be_clamped() {
    // Rule 8. Animation dt is clamped to MAX_ANIM_DT per frame, so a
    // larger step silently under-integrates instead of failing.
    let mut harness = UiHarness::new(SURFACE);
    harness.advance_frames(3, Duration::from_millis(16), button);
    assert_eq!(harness.time, Duration::from_millis(48));

    let clamped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        UiHarness::new(SURFACE).advance_frames(1, Duration::from_millis(500), button);
    }));
    assert!(clamped.is_err(), "an over-MAX_ANIM_DT step must panic");
}

#[test]
fn frame_without_baseline_forces_a_full_repaint() {
    use crate::ui::frame_report::FramePaint;

    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    // A steady frame with no input has nothing to repaint.
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);
    // Dropping the baseline forces the whole surface.
    assert_eq!(
        harness.frame_without_baseline(button).paint(),
        FramePaint::Full,
    );
}
