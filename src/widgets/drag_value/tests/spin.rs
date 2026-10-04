//! The focused chip as a spin button: focus opens no editor, the arrows
//! step, and a typed character replaces the value.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::drag_value::tests::support::deferred_frame;
use crate::widgets::drag_value::{DragValue, DragValueState};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(300, 100);

/// The press that starts a scrub focuses the chip, and the scrub still
/// writes and commits: focus alone opens no editor.
#[test]
fn a_scrub_focuses_the_chip_and_stays_a_scrub() {
    let id = WidgetId::from_hash("dv-spin-scrub");
    let mut h = UiHarness::new(SURFACE);
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.press_at(Vec2::new(50.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.release();
    deferred_frame(&mut h, id, &mut canonical, true, false);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(h.focus(), Some(id), "the press focused the chip");
    assert!(
        !matches!(
            h.state::<DragValueState>(id),
            DragValueState::Editing { .. }
        ),
        "and opened no editor",
    );
    assert_eq!(canonical, 30.0, "20 px at speed 1");
}

/// A focused chip steps by one unit of its last decimal — 0.01 at two
/// decimals — and ten with Shift, each step one commit, held by the
/// range: 5 → 5.01 → 5.11 → 5.10, then End-ward steps stop at 5.15.
/// An integer binding steps by one.
#[test]
fn the_arrows_step_a_focused_chip() {
    let id = WidgetId::from_hash("dv-spin-step");
    let mut h = UiHarness::new(SURFACE);
    let mut value = 5.0_f64;
    let frame = |h: &mut UiHarness, value: &mut f64| {
        h.frame_value(|ui| {
            DragValue::new(&mut *value)
                .range(0.0..=5.15)
                .decimals(2)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .id(id)
                .show(ui)
                .committed
        })
    };
    frame(&mut h, &mut value);
    h.set_focus(id);
    frame(&mut h, &mut value);
    for (mods, key, want) in [
        (Modifiers::NONE, Key::ArrowUp, 5.01),
        (Modifiers::SHIFT, Key::ArrowUp, 5.11),
        (Modifiers::NONE, Key::ArrowDown, 5.10),
        (Modifiers::SHIFT, Key::ArrowUp, 5.15),
    ] {
        h.set_modifiers(mods);
        h.key(key);
        assert!(frame(&mut h, &mut value), "{mods:?} {key:?} commits");
        assert_eq!(value, want, "{mods:?} {key:?}");
    }

    let int_id = WidgetId::from_hash("dv-spin-int");
    let mut count = 7_i64;
    h.set_modifiers(Modifiers::NONE);
    let record = |h: &mut UiHarness, count: &mut i64| {
        h.frame(|ui| {
            DragValue::new(&mut *count).id(int_id).size(40.0).show(ui);
        });
    };
    record(&mut h, &mut count);
    h.set_focus(int_id);
    record(&mut h, &mut count);
    h.key(Key::ArrowUp);
    record(&mut h, &mut count);
    assert_eq!(count, 8, "an integer steps by one");
}

/// A character typed into the focused chip opens the editor on that
/// frame and replaces the value with itself; Enter commits it and leaves
/// the chip focused, so the arrows step on from the typed value.
#[test]
fn typing_into_the_focused_chip_replaces_the_value() {
    let id = WidgetId::from_hash("dv-spin-type");
    let mut h = UiHarness::new(SURFACE);
    let mut canonical = 5.0_f64;
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.set_focus(id);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    h.key(Key::Char('7'));
    deferred_frame(&mut h, id, &mut canonical, true, false);
    match h.state::<DragValueState>(id) {
        DragValueState::Editing { buffer, .. } => assert_eq!(buffer, "7"),
        state => panic!("the typed character opened no editor: {state:?}"),
    }
    h.key(Key::Enter);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 7.0);
    assert_eq!(h.focus(), Some(id), "the chip keeps focus");
    h.key(Key::ArrowUp);
    deferred_frame(&mut h, id, &mut canonical, true, false);
    assert_eq!(canonical, 7.01);
}
