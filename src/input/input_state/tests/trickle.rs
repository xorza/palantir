//! Gestures fed between two frames, spread one change of a kind per
//! frame by `InputQueue` — each case stepped frame by frame so the
//! spread itself is what is asserted.

use crate::Ui;
use crate::input::capture::DRAG_THRESHOLD;
use crate::input::keyboard::key::Key;
use crate::input::response::button_phase::ButtonPhase;
use crate::input::response::button_state::ButtonState;
use crate::input::response::drag::Drag;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};
use std::time::Duration;

const TARGET: Vec2 = Vec2::new(50.0, 50.0);

fn target() -> WidgetId {
    WidgetId::from_hash("target")
}

fn scene(ui: &mut Ui) {
    Panel::hstack().auto_id().show(ui, |ui| {
        Panel::zstack()
            .id(target())
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::DRAG)
            .show(ui, |_| {});
    });
}

/// Step frames while input owes one — the report asks for the next
/// frame while a replayed or held event is still to be seen — collecting
/// pass A's left-button state for each.
fn step_until_idle(h: &mut UiHarness) -> Vec<ButtonState> {
    let mut frames = Vec::new();
    loop {
        let state = std::cell::Cell::new(None);
        let report = h.step(|ui| {
            scene(ui);
            if state.get().is_none() {
                state.set(Some(ui.response_for(target()).left));
            }
        });
        frames.push(state.get().expect("the frame recorded"));
        if !report.repaint_requested {
            return frames;
        }
    }
}

fn down(count: u8) -> ButtonState {
    ButtonState::new(ButtonPhase::Down { count }, Drag::None)
}

fn up_click(count: u8) -> ButtonState {
    ButtonState::new(ButtonPhase::Up { click: Some(count) }, Drag::None)
}

/// Each batch, fed between two frames, against the frames it lands in.
/// Hand-derived: a press frame shows `Down`, a release frame `Up{click}`
/// (the click carrying its run number), and no frame holds both.
#[test]
fn each_batch_lands_one_button_change_per_frame() {
    // [P]: one frame, held.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.press_at(TARGET);
    assert_eq!(step_until_idle(&mut h), [down(1)]);

    // [P, R]: press frame, then the click.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.click_at(TARGET);
    assert_eq!(step_until_idle(&mut h), [down(1), up_click(1)]);

    // [P, R, P, R]: four frames, and the second click is a double-click —
    // the arrival times, not the frames, decide the run.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.click_at(TARGET);
    h.click_at(TARGET);
    assert_eq!(
        step_until_idle(&mut h),
        [down(1), up_click(1), down(2), up_click(2)],
    );
}

/// The batch that used to fail `ButtonState::new`'s invariant: a drag
/// stopped and a fresh press, fed together. Spread, the stop and the new
/// press land in different frames and every state is a legal pair.
#[test]
fn a_drag_stop_and_a_new_press_never_share_a_frame() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.press_at(TARGET);
    h.step(scene);
    h.drag_to(TARGET + Vec2::new(DRAG_THRESHOLD * 4.0, 0.0));
    h.step(scene);
    h.release();
    h.press_at(TARGET);
    let frames = step_until_idle(&mut h);
    assert_eq!(frames.len(), 2, "the stop, then the press");
    assert!(
        frames[0].drag.stopped(),
        "frame 1 reports the stop: {:?}",
        frames[0]
    );
    assert_eq!(frames[1], down(1), "frame 2 the fresh press, no stale stop");
}

/// A click elsewhere between two clicks on one widget breaks the run:
/// the third press is a single, though it lands inside the window and
/// radius of the first.
#[test]
fn a_press_elsewhere_breaks_the_click_run() {
    let mut h = UiHarness::new(UVec2::new(400, 200));
    h.prime(2, scene);
    h.click_at(TARGET);
    h.advance(Duration::from_millis(100));
    h.click_at(Vec2::new(300.0, 150.0));
    h.advance(Duration::from_millis(100));
    h.press_at(TARGET);
    let frames = step_until_idle(&mut h);
    assert_eq!(frames.last(), Some(&down(1)), "{frames:?}");
}

/// A right-click between two left-clicks breaks the left run too.
#[test]
fn another_buttons_press_breaks_the_click_run() {
    use crate::input::pointer::PointerButton;
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.click_at(TARGET);
    h.click_button_at(PointerButton::Right, TARGET);
    h.press_at(TARGET);
    let frames = step_until_idle(&mut h);
    assert_eq!(frames.last(), Some(&down(1)), "{frames:?}");
}

fn field() -> WidgetId {
    WidgetId::from_hash("field")
}

#[derive(Clone, Copy, Debug, Default)]
struct FieldPass {
    submitted: bool,
}

fn record_field(ui: &mut Ui, buffer: &mut String) -> FieldPass {
    Panel::hstack()
        .auto_id()
        .show(ui, |ui| {
            let r = TextEdit::new(buffer)
                .id(field())
                .size((Sizing::fixed(200.0), Sizing::fixed(30.0)))
                .show(ui);
            FieldPass {
                submitted: r.submitted,
            }
        })
        .inner
}

/// `[Escape, 'a']` fed together: Escape blurs the field in its frame,
/// and `a` lands in the next frame, where the field no longer has focus
/// — so it is not typed into the field Escape just left.
#[test]
fn a_key_after_escape_reaches_the_next_focus_owner() {
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut buffer = String::from("hello");
    h.prime(2, |ui| {
        record_field(ui, &mut buffer);
    });
    h.set_focus(field());
    h.frame(|ui| {
        record_field(ui, &mut buffer);
    });
    h.key(Key::Escape);
    h.key(Key::Char('a'));
    let escape = h.step(|ui| {
        record_field(ui, &mut buffer);
    });
    assert!(escape.repaint_requested, "`a` is owed the next frame");
    h.step(|ui| {
        record_field(ui, &mut buffer);
    });
    assert_eq!(buffer, "hello", "the blurred field typed nothing");
    assert_eq!(h.focused_id(), None);
}

/// `[Enter, 'x']` fed together: the caller reads the submitted value in
/// Enter's frame, and `x` is typed in the next.
#[test]
fn a_key_after_enter_is_typed_after_the_submit() {
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut buffer = String::from("hello");
    h.prime(2, |ui| {
        record_field(ui, &mut buffer);
    });
    h.set_focus(field());
    h.frame(|ui| {
        record_field(ui, &mut buffer);
    });
    h.key(Key::End);
    h.frame(|ui| {
        record_field(ui, &mut buffer);
    });
    h.key(Key::Enter);
    h.key(Key::Char('x'));
    let mut enter = FieldPass::default();
    h.step(|ui| {
        let pass = record_field(ui, &mut buffer);
        enter.submitted |= pass.submitted;
    });
    assert!(enter.submitted);
    assert_eq!(buffer, "hello", "the submitted value is the one before `x`");
    h.step(|ui| {
        record_field(ui, &mut buffer);
    });
    assert_eq!(buffer, "hellox", "`x` is typed in the frame after");
}

/// A capture whose widget vanishes mid-drag ends with a stop edge, and
/// the frame that carries it records instead of painting from the
/// retained tree — the report asks for it.
#[test]
fn an_evicted_drag_asks_for_the_frame_that_reports_its_stop() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.press_at(TARGET);
    h.step(scene);
    h.drag_to(TARGET + Vec2::new(DRAG_THRESHOLD * 4.0, 0.0));
    h.step(scene);
    let gone = h.step(|ui| {
        Panel::hstack().auto_id().show(ui, |_| {});
    });
    assert!(
        gone.repaint_requested,
        "the eviction's stop edge owes a frame and the report asks for it",
    );
}
