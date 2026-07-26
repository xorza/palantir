//! Which pointer edges buy a same-frame settle — a second record pass —
//! and which do not. The pass *count* is the contract:
//! `InputState::frame_had_action` is what `Ui::frame` reads to decide, and
//! the second pass is a whole re-record, so a spurious settle roughly
//! doubles the frame. See `RELAYOUT.md` §4-B.
//!
//! The two deliberately-narrow arms are the point of this file: a bare
//! press and a `ReleaseKind::Miss` write only state their own target
//! reads, so neither qualifies, while a click, a drag stop, a drag latch,
//! and any `PointerWake::BUTTONS` subscriber all do.

use std::time::Duration;

use glam::{UVec2, Vec2};

use crate::Ui;
use crate::display::Display;
use crate::input::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::DRAG_THRESHOLD;
use crate::input::watch::PointerWake;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::widgets::button::Button;

/// Bigger than the button, so the pointer has inert surface to sit on.
const SURFACE: UVec2 = UVec2::new(200, 200);

fn button_id() -> WidgetId {
    WidgetId::from_hash("settle-button")
}

fn button(ui: &mut Ui) {
    let _ = Button::new()
        .id(button_id())
        .label("x")
        .size((100.0, 100.0))
        .show(ui);
}

fn button_watching_buttons(ui: &mut Ui) {
    button(ui);
    ui.watch_pointer(PointerWake::BUTTONS);
}

/// One warm frame of `record`, plus the button's arranged rect — the
/// press/release coordinates are derived from it rather than assumed, so
/// the tests don't silently drift with root alignment.
fn warm(record: fn(&mut Ui)) -> (Ui, Rect) {
    let mut ui = Ui::for_test();
    ui.run_at(SURFACE, record);
    let rect = ui
        .response_for(button_id())
        .rect
        .expect("the button arranged on the warm frame");
    (ui, rect)
}

/// Record passes the next frame runs — 1 for no settle, 2 for a settle.
fn passes(ui: &mut Ui, record: fn(&mut Ui)) -> usize {
    let mut n = 0;
    let _ = ui.record_test_frame_without_baseline(
        Display::from_physical(SURFACE, 1.0),
        Duration::from_millis(16),
        |ui| {
            n += 1;
            record(ui);
        },
    );
    n
}

#[test]
fn a_bare_press_does_not_settle_but_a_watched_one_does() {
    // The capture reaches only the press target and `focused` is read
    // live, so nothing recorded earlier in the pass can be stale.
    let (mut ui, rect) = warm(button);
    ui.press_at(rect.center());
    assert_eq!(
        passes(&mut ui, button),
        1,
        "a press on a button settles nothing"
    );

    // Same press, but a `BUTTONS` subscriber makes the write opaque —
    // this is the arm `Modal` relies on to dismiss itself.
    let (mut ui, rect) = warm(button_watching_buttons);
    ui.press_at(rect.center());
    assert_eq!(
        passes(&mut ui, button_watching_buttons),
        2,
        "a BUTTONS subscriber cannot be reasoned about, so it settles",
    );

    // A press that hits nothing at all still settles nothing, and (unlike
    // the two above) does not even need to record.
    let (mut ui, _) = warm(button);
    ui.press_at(Vec2::new(180.0, 180.0));
    assert_eq!(
        passes(&mut ui, button),
        1,
        "a press on inert surface settles nothing"
    );
}

#[test]
fn a_missed_release_does_not_settle_but_a_click_does() {
    // Release back on the press target: `ReleaseKind::Click`. Apps act on
    // this edge, so it keeps its settle.
    let (mut ui, rect) = warm(button);
    ui.press_at(rect.center());
    ui.release_left();
    assert_eq!(passes(&mut ui, button), 2, "a click settles");

    // Slip off the button and release: `ReleaseKind::Miss`. The travel is
    // under DRAG_THRESHOLD so no drag latches — otherwise this would be a
    // `DragStopped` and settle for a different reason.
    let (mut ui, rect) = warm(button);
    let edge = Vec2::new(rect.max().x - 1.0, rect.center().y);
    let off = edge + Vec2::new(DRAG_THRESHOLD - 1.0, 0.0);
    assert!(
        !rect.contains(off) && edge.distance(off) < DRAG_THRESHOLD,
        "the probe must leave the button without latching a drag: {rect:?} → {off:?}",
    );
    ui.press_at(edge);
    ui.on_input(InputEvent::PointerMoved(off));
    ui.release_left();
    assert_eq!(
        passes(&mut ui, button),
        1,
        "a miss fires no click and settles nothing"
    );
}

#[test]
fn a_drag_settles_on_its_latch_and_again_on_its_stop() {
    let (mut ui, rect) = warm(button);
    let origin = rect.center();

    // Frame 1 of the gesture: crossing the threshold latches the drag,
    // which is its own settle arm (`PointerMoved`, not the press).
    ui.press_at(origin);
    ui.on_input(InputEvent::PointerMoved(
        origin + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0),
    ));
    assert_eq!(passes(&mut ui, button), 2, "the latch settles");

    // Frame 2: the release is `DragStopped` on its own, with no latch in
    // the batch to mask it.
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    assert_eq!(passes(&mut ui, button), 2, "the drag stop settles");
}
