//! Which pointer edges buy a same-frame settle — a second record pass —
//! and which do not. The pass *count* is the contract:
//! `InputState::frame_had_action` is what `Ui::frame` reads to decide, and
//! the second pass is a whole re-record, so a spurious settle roughly
//! doubles the frame.
//!
//! The two deliberately-narrow arms are the point of this file: a bare
//! press and a `ReleaseKind::Miss` write only state their own target
//! reads, so neither qualifies, while a click, a drag stop, a drag latch,
//! and any `PointerWake::BUTTONS` subscriber all do.

use std::time::Duration;

use glam::{UVec2, Vec2};

use crate::Ui;
use crate::input::capture::DRAG_THRESHOLD;
use crate::input::watch::PointerWake;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
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
fn warm(record: fn(&mut Ui)) -> (UiHarness, Rect) {
    let mut h = UiHarness::new(SURFACE);
    h.frame(record);
    let rect =
        h.ui.response_for(button_id())
            .rect
            .expect("the button arranged on the warm frame");
    (h, rect)
}

/// Record passes the next frame runs — 1 for no settle, 2 for a settle.
fn passes(h: &mut UiHarness, record: fn(&mut Ui)) -> usize {
    let mut n = 0;
    let _ = h.at(Duration::from_millis(16)).frame(|ui| {
        n += 1;
        record(ui);
    });
    n
}

#[test]
fn a_bare_press_does_not_settle_but_a_watched_one_does() {
    // The capture reaches only the press target and `focused` is read
    // live, so nothing recorded earlier in the pass can be stale.
    let (mut h, rect) = warm(button);
    h.press_at(rect.center());
    assert_eq!(
        passes(&mut h, button),
        1,
        "a press on a button settles nothing"
    );

    // Same press, but a `BUTTONS` subscriber makes the write opaque —
    // this is the arm `Modal` relies on to dismiss itself.
    let (mut h, rect) = warm(button_watching_buttons);
    h.press_at(rect.center());
    assert_eq!(
        passes(&mut h, button_watching_buttons),
        2,
        "a BUTTONS subscriber cannot be reasoned about, so it settles",
    );

    // A press that hits nothing at all still settles nothing, and (unlike
    // the two above) does not even need to record.
    let (mut h, _) = warm(button);
    h.press_at(Vec2::new(180.0, 180.0));
    assert_eq!(
        passes(&mut h, button),
        1,
        "a press on inert surface settles nothing"
    );
}

#[test]
fn a_missed_release_does_not_settle_but_a_click_does() {
    // Release back on the press target: `ReleaseKind::Click`. Apps act on
    // this edge, so it keeps its settle.
    let (mut h, rect) = warm(button);
    h.press_at(rect.center());
    h.release();
    assert_eq!(passes(&mut h, button), 2, "a click settles");

    // Slip off the button and release: `ReleaseKind::Miss`. The travel is
    // under DRAG_THRESHOLD so no drag latches — otherwise this would be a
    // `DragStopped` and settle for a different reason.
    let (mut h, rect) = warm(button);
    let edge = Vec2::new(rect.max().x - 1.0, rect.center().y);
    let off = edge + Vec2::new(DRAG_THRESHOLD - 1.0, 0.0);
    assert!(
        !rect.contains(off) && edge.distance(off) < DRAG_THRESHOLD,
        "the probe must leave the button without latching a drag: {rect:?} → {off:?}",
    );
    h.press_at(edge);
    h.move_to(off);
    h.release();
    assert_eq!(
        passes(&mut h, button),
        1,
        "a miss fires no click and settles nothing"
    );
}

/// The tally behind the frame-stats overlay's `settle n/m`, which is how
/// a real gesture's cost gets read in the running app. A sustained drag is
/// the case that matters: after the latch frame, holding and moving must
/// cost exactly one record pass each.
#[test]
fn a_sustained_drag_tallies_one_settle_for_its_latch_and_none_after() {
    let (mut h, rect) = warm(button);
    let origin = rect.center();
    let (base_settles, base_records) = (h.ui.frame_runtime().settle_frames, h.ui.frame_id());

    h.press_at(origin);
    h.move_to(origin + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    let _ = passes(&mut h, button);

    // Eight more frames of holding and moving — the whole gesture body.
    for step in 2..10 {
        h.move_to(origin + Vec2::new(DRAG_THRESHOLD + step as f32, 0.0));
        let _ = passes(&mut h, button);
    }

    assert_eq!(
        h.ui.frame_id() - base_records,
        9,
        "nine full-record frames were driven",
    );
    assert_eq!(
        h.ui.frame_runtime().settle_frames - base_settles,
        1,
        "only the threshold-crossing frame settles; the drag body is free",
    );
}

#[test]
fn a_drag_settles_on_its_latch_and_again_on_its_stop() {
    let (mut h, rect) = warm(button);
    let origin = rect.center();

    // Frame 1 of the gesture: crossing the threshold latches the drag,
    // which is its own settle arm (`PointerMoved`, not the press).
    h.press_at(origin);
    h.move_to(origin + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    assert_eq!(passes(&mut h, button), 2, "the latch settles");

    // Frame 2: the release is `DragStopped` on its own, with no latch in
    // the batch to mask it.
    h.release();
    assert_eq!(passes(&mut h, button), 2, "the drag stop settles");
}
