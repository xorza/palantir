//! Which pointer edges buy a same-frame settle (a second record pass): click, drag stop/latch and
//! `PointerWake::BUTTONS` subscribers do; a bare press and `ReleaseKind::Miss` do not.

use std::time::Duration;

use glam::{UVec2, Vec2};

use crate::Ui;
use crate::input::capture::DRAG_THRESHOLD;
use crate::input::watch::PointerWake;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;

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

#[derive(Debug)]
struct Warm {
    h: UiHarness,
    button: Rect,
}

fn warm(record: fn(&mut Ui)) -> Warm {
    let mut h = UiHarness::new(SURFACE);
    h.frame(record);
    let rect = h
        .rect(button_id())
        .expect("the button arranged on the warm frame");
    Warm { h, button: rect }
}

/// Record passes the next frame (16 ms on) runs: 1 for no settle, 2 for a settle.
fn passes(h: &mut UiHarness, record: fn(&mut Ui)) -> usize {
    let mut n = 0;
    let _ = h.advance(Duration::from_millis(16)).frame(|ui| {
        n += 1;
        record(ui);
    });
    n
}

#[test]
fn a_bare_press_does_not_settle_but_a_watched_one_does() {
    let Warm {
        mut h,
        button: rect,
    } = warm(button);
    h.press_at(rect.center());
    assert_eq!(
        passes(&mut h, button),
        1,
        "a press on a button settles nothing"
    );

    // A `BUTTONS` subscriber makes the write opaque; `Modal` relies on this to dismiss itself.
    let Warm {
        mut h,
        button: rect,
    } = warm(button_watching_buttons);
    h.press_at(rect.center());
    assert_eq!(
        passes(&mut h, button_watching_buttons),
        2,
        "a BUTTONS subscriber cannot be reasoned about, so it settles",
    );

    let Warm { mut h, .. } = warm(button);
    h.press_at(Vec2::new(180.0, 180.0));
    assert_eq!(
        passes(&mut h, button),
        1,
        "a press on inert surface settles nothing"
    );
}

#[test]
fn a_missed_release_does_not_settle_but_a_click_does() {
    // A click settles (apps act on it); the press lands a frame before its release (`InputQueue`).
    let Warm {
        mut h,
        button: rect,
    } = warm(button);
    h.press_at(rect.center());
    h.frame(button);
    h.release();
    assert_eq!(passes(&mut h, button), 2, "a click settles");

    // A miss stays under DRAG_THRESHOLD, else it would be a `DragStopped`.
    let Warm {
        mut h,
        button: rect,
    } = warm(button);
    let edge = Vec2::new(rect.max().x - 1.0, rect.center().y);
    let off = edge + Vec2::new(DRAG_THRESHOLD - 1.0, 0.0);
    assert!(
        !rect.contains(off) && edge.distance(off) < DRAG_THRESHOLD,
        "the probe must leave the button without latching a drag: {rect:?} → {off:?}",
    );
    h.press_at(edge);
    h.move_to(off);
    h.frame(button);
    h.release();
    assert_eq!(
        passes(&mut h, button),
        1,
        "a miss fires no click and settles nothing"
    );
}

/// After the latch frame, holding and moving cost exactly one record pass each (`settle n/m` overlay).
#[test]
fn a_sustained_drag_tallies_one_settle_for_its_latch_and_none_after() {
    let Warm {
        mut h,
        button: rect,
    } = warm(button);
    let origin = rect.center();
    let (base_settles, base_records) = (h.ui.frame_runtime().settle_frames, h.ui.frame_id());

    h.press_at(origin);
    h.move_to(origin + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    let _ = passes(&mut h, button);

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
    let Warm {
        mut h,
        button: rect,
    } = warm(button);
    let origin = rect.center();

    // Frame 1: crossing the threshold latches the drag, its own settle arm (`PointerMoved`).
    h.press_at(origin);
    h.move_to(origin + Vec2::new(DRAG_THRESHOLD + 1.0, 0.0));
    assert_eq!(passes(&mut h, button), 2, "the latch settles");

    // Frame 2: the release is `DragStopped` with no latch in the batch to mask it.
    h.release();
    assert_eq!(passes(&mut h, button), 2, "the drag stop settles");
}
