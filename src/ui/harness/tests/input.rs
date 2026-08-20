//! The gestures the harness synthesises, and the routing each depends on.

use crate::input::keyboard::KeyboardEvent;
use crate::layout::types::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::tests::support::{INSIDE, OUTSIDE, SURFACE, button, target};
use crate::ui::harness::*;
use crate::widgets::panel::Panel;

#[test]
fn drag_to_latches_past_the_threshold_and_panics_under_it() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    harness.press_at(INSIDE);
    let latch = harness.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    // The move helpers hand back `on_input`'s own `InputDelta`, so
    // asserting on the repaint hint never needs the raw door. A latching
    // drag is exactly the "state the next frame has to show" case.
    assert!(
        latch.requests_repaint,
        "the latching move must report a repaint, same as on_input would",
    );
    assert!(
        harness.response_in(target(), button).left.drag.started(),
        "travel past DRAG_THRESHOLD latches a drag",
    );
    harness.release();

    // The value is the event's, not a stub: leaving the button crosses a
    // hover boundary and reports a repaint, while a second move over the
    // same bare surface crosses nothing and reports none.
    assert!(
        harness.move_to(OUTSIDE).requests_repaint,
        "leaving the button is a hover crossing",
    );
    assert!(
        !harness.move_to(OUTSIDE + Vec2::splat(1.0)).requests_repaint,
        "a second move over bare surface crosses nothing",
    );

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

    // `press` latches the same origin as `press_at` — it reads the
    // pointer back out of `InputState` instead of being handed one, so a
    // press separated from its move still arms the threshold check.
    let mut split = UiHarness::new(SURFACE);
    split.prime(2, button);
    split.move_to(INSIDE);
    assert!(
        split.press().requests_repaint,
        "a press latching on the button is a repaint",
    );
    split.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    assert!(
        split.response_in(target(), button).left.drag.started(),
        "move-then-press arms drag_to exactly as press_at does",
    );

    // Without a pointer position there is no origin to measure from, so
    // the threshold check must refuse rather than invent one.
    let never_moved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut harness = UiHarness::new(SURFACE);
        harness.prime(2, button);
        harness.press();
        harness.drag_to(INSIDE);
    }));
    assert!(
        never_moved.is_err(),
        "a press with the pointer nowhere cannot arm a drag",
    );
}

#[test]
fn modifiers_are_sticky_until_set_back() {
    // Rule 13. `ModifiersChanged` carries a snapshot that persists, so
    // every later key inherits it until something sets it back.
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let mut harness = UiHarness::new(SURFACE);

    // The set under test is `InputState`'s, not a copy on the harness —
    // a copy desyncs the moment one modifier goes through `on_input`,
    // and `set_modifiers` would then suppress the emit that clears it.
    harness.set_modifiers(ctrl);
    harness.key(Key::Char('b'));
    assert_eq!(
        harness.ui.input.modifiers, ctrl,
        "the key does not consume the chord",
    );

    harness.set_modifiers(Modifiers::NONE);
    harness.key(Key::Char('c'));
    assert_eq!(
        harness.ui.input.modifiers,
        Modifiers::NONE,
        "…and it stays cleared once set back",
    );

    // Mixing the raw door with the helper must stay coherent: the raw
    // event moves the real set, so the helper still sees a change and
    // emits the clearing event rather than leaving ctrl silently held.
    harness.on_input(InputEvent::ModifiersChanged(ctrl));
    harness.set_modifiers(Modifiers::NONE);
    assert_eq!(
        harness.ui.input.modifiers,
        Modifiers::NONE,
        "set_modifiers clears a set the raw door installed",
    );
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

    // `scroll_lines_at` is `move_to` + `scroll_lines`, so the bare form
    // aims at wherever the pointer was left — here still OUTSIDE, which
    // is why a separate `move_to` is what re-aims it.
    harness.scroll_lines(Vec2::new(0.0, 3.0));
    assert_eq!(
        harness.response_in(scroller, build).scroll.lines.y,
        0.0,
        "a bare scroll inherits the last position, it does not re-aim",
    );

    harness.move_to(INSIDE);
    harness.scroll_lines(Vec2::new(0.0, 3.0));
    assert_eq!(
        harness.response_in(scroller, build).scroll.lines.y,
        3.0,
        "…and lands once the pointer is moved onto the target",
    );

    // Pinch routes by the same last-position rule; `pinch_at` returns
    // the zoom's own delta, not the positioning move's.
    let zoomer = WidgetId::from_hash("zoomer");
    let zoom_build = |ui: &mut Ui| {
        Panel::hstack()
            .id(zoomer)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .sense(Sense::PINCH)
            .show(ui, |_| {});
    };
    let mut pinched = UiHarness::new(SURFACE);
    pinched.prime(2, zoom_build);
    assert!(
        pinched.pinch_at(INSIDE, 1.5).requests_repaint,
        "a pinch that lands on a zoom target wakes the next frame",
    );
    assert_eq!(
        pinched.response_in(zoomer, zoom_build).scroll.zoom,
        1.5,
        "the zoom factor reaches the widget under the pointer",
    );
}
