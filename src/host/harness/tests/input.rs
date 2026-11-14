//! The gestures the harness synthesizes, and the routing each depends on.

use crate::host::harness::tests::support::{self, INSIDE, OUTSIDE, SURFACE};
use crate::host::harness::*;
use crate::internals::panic_probe;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

#[test]
fn drag_to_latches_past_the_threshold_and_panics_under_it() {
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);

    harness.press(INSIDE, PointerButton::Left);
    let latch = harness.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    // The move helpers return `on_input`'s own `InputDelta`, so the repaint hint needs no raw door;
    // a latching drag is the "state the next frame has to show" case.
    assert!(
        latch.repaint_requested,
        "the latching move must report a repaint, same as on_input would",
    );
    assert!(
        harness
            .response_in(support::target(), support::button)
            .left
            .drag
            .started(),
        "travel past DRAG_THRESHOLD latches a drag",
    );
    harness.release(PointerButton::Left);

    // The value is the event's, not a stub: leaving the button crosses a hover boundary and repaints,
    // a second move over bare surface crosses nothing and doesn't.
    assert!(
        harness.hover(OUTSIDE).repaint_requested,
        "leaving the button is a hover crossing",
    );
    assert!(
        !harness.hover(OUTSIDE + Vec2::splat(1.0)).repaint_requested,
        "a second move over bare surface crosses nothing",
    );

    // Sub-threshold travel must panic, not quietly fail to latch.
    panic_probe::assert_panics_with("drag threshold; no drag would start", || {
        let mut harness = Harness::mono(SURFACE);
        harness.prime(2, support::button);
        harness.press(INSIDE, PointerButton::Left);
        harness.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD - 1.0, 0.0));
    });
    panic_probe::assert_panics_with("drag_to needs a press first", || {
        Harness::mono(SURFACE).drag_to(INSIDE);
    });

    // A raw press latches the same origin as `press` (it reads the pointer back out of
    // `Input`), so a press separated from its move still arms the threshold check.
    let mut split = Harness::mono(SURFACE);
    split.prime(2, support::button);
    split.hover(INSIDE);
    assert!(
        split
            .on_input(InputEvent::PointerPressed(PointerButton::Left))
            .repaint_requested,
        "a press latching on the button is a repaint",
    );
    split.drag_to(INSIDE + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    assert!(
        split
            .response_in(support::target(), support::button)
            .left
            .drag
            .started(),
        "move-then-press arms drag_to exactly as press does",
    );

    // Without a pointer position there is no origin to measure from, so
    // the threshold check must refuse rather than invent one.
    panic_probe::assert_panics_with("drag_to needs a press on the window", || {
        let mut harness = Harness::mono(SURFACE);
        harness.prime(2, support::button);
        harness.on_input(InputEvent::PointerPressed(PointerButton::Left));
        harness.drag_to(INSIDE);
    });
}

#[test]
fn modifiers_are_sticky_until_set_back() {
    // `ModifiersChanged` carries a snapshot that persists, so every later key inherits it until
    // something sets it back.
    let ctrl = Modifiers::CTRL;
    let mut harness = Harness::mono(SURFACE);

    // The set under test is `Input`'s, not a harness copy: a copy desyncs once one modifier goes
    // through `on_input`, and `hold_modifiers` would then suppress the emit that clears it.
    harness.hold_modifiers(ctrl);
    harness.key(Key::Char('b'));
    assert_eq!(
        harness.ui().input().peek_modifiers(),
        ctrl,
        "the key does not consume the chord",
    );

    harness.hold_modifiers(Modifiers::NONE);
    harness.key(Key::Char('c'));
    assert_eq!(
        harness.ui().input().peek_modifiers(),
        Modifiers::NONE,
        "…and it stays cleared once set back",
    );

    // Mixing the raw door with the helper stays coherent: the raw event moves the real set, so the
    // helper still sees a change and emits the clearing event instead of leaving ctrl held.
    harness.on_input(InputEvent::ModifiersChanged(ctrl));
    harness.hold_modifiers(Modifiers::NONE);
    assert_eq!(
        harness.ui().input().peek_modifiers(),
        Modifiers::NONE,
        "hold_modifiers clears a set the raw door installed",
    );
}

#[test]
fn typed_text_arrives_as_one_press_per_char() {
    // `type_text` emits the `KeyDown { Key::Char }` a real window produces, which is the only path
    // a field inserts from; a newline is Enter, and a control character no key types refuses
    // the text (`driver::play::typed_key` holds the whole map).
    let mut harness = Harness::mono(SURFACE);
    // Rule 11: keyboard events are dropped at ingress when nothing holds focus and no subscriber
    // matches, so focus comes first.
    harness.ui_mut().set_focus(support::target());

    harness.type_text("hi\n");

    let keys: Vec<_> = harness
        .ui()
        .input()
        .keyboard_events_at(Layer::Main.into())
        .iter()
        .map(|press| press.key)
        .collect();

    assert_eq!(keys, [Key::Char('h'), Key::Char('i'), Key::Enter]);
    panic_probe::assert_panics_with("is a control character no key types", || {
        Harness::mono(SURFACE).type_text("\u{1b}");
    });
}

#[test]
fn scroll_routes_to_whatever_the_pointer_moved_over() {
    // Scroll carries no position; the target is the last hover.
    let scroller = WidgetId::from_hash("scroller");
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(scroller)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .sense(Sense::SCROLL)
            .show(ui, |_| {});
    };

    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, build);

    harness.scroll_lines(INSIDE, Vec2::new(0.0, 3.0));
    let hit = harness.response_in(scroller, build);
    assert_eq!(hit.scroll.lines.y, 3.0, "the hovered widget got the delta");

    harness.scroll_lines(OUTSIDE, Vec2::new(0.0, 3.0));
    let missed = harness.response_in(scroller, build);
    assert_eq!(
        missed.scroll.lines.y, 0.0,
        "a scroll over bare surface reaches nobody",
    );

    // The raw event aims at wherever the pointer was left, still OUTSIDE; only a hover re-aims it.
    harness.on_input(InputEvent::scroll_lines(Vec2::new(0.0, 3.0)));
    assert_eq!(
        harness.response_in(scroller, build).scroll.lines.y,
        0.0,
        "a bare scroll inherits the last position, it does not re-aim",
    );

    harness.hover(INSIDE);
    harness.on_input(InputEvent::scroll_lines(Vec2::new(0.0, 3.0)));
    assert_eq!(
        harness.response_in(scroller, build).scroll.lines.y,
        3.0,
        "…and lands once the pointer is moved onto the target",
    );

    // Pinch routes by the same last-position rule; `pinch` returns the zoom's own delta, not the
    // positioning move's.
    let zoomer = WidgetId::from_hash("zoomer");
    let zoom_build = |ui: &mut Ui| {
        Panel::hstack()
            .id(zoomer)
            .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
            .sense(Sense::PINCH)
            .show(ui, |_| {});
    };
    let mut pinched = Harness::mono(SURFACE);
    pinched.prime(2, zoom_build);
    assert!(
        pinched
            .pinch(INSIDE, ZoomFactor::new(1.5).unwrap())
            .repaint_requested,
        "a pinch that lands on a zoom target wakes the next frame",
    );
    assert_eq!(
        pinched.response_in(zoomer, zoom_build).scroll.zoom.get(),
        1.5,
        "the zoom factor reaches the widget under the pointer",
    );
}
