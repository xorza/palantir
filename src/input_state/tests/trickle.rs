//! Gestures fed between two frames are spread one change of a kind per frame by `InputQueue`; each case steps frame by frame.

use crate::input::input_event::InputEvent;
use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::interaction::button_state::ButtonState;
use crate::input::interaction::drag::Drag;
use crate::input::interaction::pointer_edge::PointerEdge;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::sense::Sense;
use crate::input::watch::PointerWake;
use crate::input_state::capture::DRAG_THRESHOLD;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::text_edit::edit_end::EditEnd;
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

/// Step frames recording `record` while input owes one, keeping what `sample` read in each frame's
/// first pass.
fn sample_until_idle<T>(
    h: &mut UiHarness,
    mut record: impl FnMut(&mut Ui),
    mut sample: impl FnMut(&mut Ui) -> T,
) -> Vec<T> {
    let mut frames = Vec::new();
    loop {
        let mut seen = None;
        let report = h.step(|ui| {
            record(ui);
            if seen.is_none() {
                seen = Some(sample(ui));
            }
        });
        frames.push(seen.expect("the frame recorded"));
        if !report.repaint_requested {
            return frames;
        }
    }
}

/// The target's left-button state per frame, until input is idle.
fn step_until_idle(h: &mut UiHarness) -> Vec<ButtonState> {
    sample_until_idle(h, scene, |ui| ui.response_for(target()).left)
}

fn down(count: u8) -> ButtonState {
    ButtonState::new(ButtonPhase::Down { count }, Drag::None)
}

fn up_click(count: u8) -> ButtonState {
    ButtonState::new(ButtonPhase::Up { click: Some(count) }, Drag::None)
}

/// Each batch fed between two frames against the frames it lands in: a press frame shows `Down`, a release frame `Up{click}` (carrying its run number), never both.
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

    // [P, R, P, R]: four frames; arrival times, not frames, decide the double-click.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.click_at(TARGET);
    h.click_at(TARGET);
    assert_eq!(
        step_until_idle(&mut h),
        [down(1), up_click(1), down(2), up_click(2)],
    );
}

/// A drag stop and a fresh press fed together would break `ButtonState::new`'s invariant in one frame; spread, each state is a legal pair.
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

/// A click elsewhere between two clicks on one widget breaks the run, though the third press is inside the window and radius.
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

#[derive(Clone, Copy, Debug)]
struct FieldPass {
    ended: Option<EditEnd>,
}

fn record_field(ui: &mut Ui, buffer: &mut String) -> FieldPass {
    Panel::hstack()
        .auto_id()
        .show(ui, |ui| {
            let r = TextEdit::new(buffer)
                .id(field())
                .size((Sizing::fixed(200.0), Sizing::fixed(30.0)))
                .show(ui);
            FieldPass { ended: r.ended }
        })
        .inner
}

/// `[Escape, 'a']`: Escape blurs the field in its frame, so `a` lands next frame unfocused and isn't typed in.
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
    assert_eq!(h.focus(), None);
}

/// `[Enter, 'x']`: the caller reads the submitted value in Enter's frame; `x` is typed in the next.
/// Committed IME text is typed text, so `[Enter, ImeCommit("x")]` lands the same way.
#[test]
fn a_key_after_enter_is_typed_after_the_submit() {
    for commit in [false, true] {
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
        if commit {
            h.on_input(InputEvent::ImeCommit("x"));
        } else {
            h.key(Key::Char('x'));
        }
        let enter = h.step_passes(|ui| record_field(ui, &mut buffer));
        assert_eq!(enter.a().ended, Some(EditEnd::Submitted), "commit {commit}");
        assert_eq!(buffer, "hello", "commit {commit}: submitted before `x`");
        h.step(|ui| {
            record_field(ui, &mut buffer);
        });
        assert_eq!(
            buffer, "hellox",
            "commit {commit}: `x` typed in the frame after"
        );
    }
}

fn field_b() -> WidgetId {
    WidgetId::from_hash("field-b")
}

fn record_two_fields(ui: &mut Ui, a: &mut String, b: &mut String) {
    Panel::hstack().auto_id().show(ui, |ui| {
        TextEdit::new(a)
            .id(field())
            .size((Sizing::fixed(120.0), Sizing::fixed(30.0)))
            .show(ui);
        TextEdit::new(b)
            .id(field_b())
            .size((Sizing::fixed(120.0), Sizing::fixed(30.0)))
            .show(ui);
    });
}

#[derive(Clone, Copy, Debug)]
enum FocusMover {
    Tab,
    PressOnB,
}

/// Keys typed ahead of a focus move reach the field they were typed in: with `['a', Tab]` and
/// `['a', press on B]` fed together, `a` lands in A, and focus moves to B the frame after.
#[test]
fn keys_typed_before_a_focus_move_stay_with_their_field() {
    for mover in [FocusMover::Tab, FocusMover::PressOnB] {
        let mut h = UiHarness::new(UVec2::new(300, 100));
        let (mut a, mut b) = (String::new(), String::new());
        h.prime(2, |ui| record_two_fields(ui, &mut a, &mut b));
        h.set_focus(field());
        h.frame(|ui| record_two_fields(ui, &mut a, &mut b));
        h.key(Key::Char('a'));
        match mover {
            FocusMover::Tab => {
                h.key(Key::Tab);
            }
            FocusMover::PressOnB => h.click_on(field_b()),
        }
        h.frame(|ui| record_two_fields(ui, &mut a, &mut b));
        assert_eq!((a.as_str(), b.as_str()), ("a", ""), "{mover:?}");
        assert_eq!(h.focus(), Some(field_b()), "{mover:?}");
    }
}

/// A press reads the modifiers it arrived under: Shift released in the batch that pressed waits a
/// frame, so the press frame still holds Shift.
#[test]
fn a_press_reads_the_modifiers_it_came_under() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.set_modifiers(Modifiers::SHIFT);
    h.press_at(TARGET);
    h.set_modifiers(Modifiers::NONE);
    let seen = sample_until_idle(&mut h, scene, |ui| {
        (ui.response_for(target()).left, ui.peek_modifiers())
    });
    assert_eq!(seen.first(), Some(&(down(1), Modifiers::SHIFT)), "{seen:?}");
    assert_eq!(
        seen.last().map(|&(_, mods)| mods),
        Some(Modifiers::NONE),
        "{seen:?}"
    );
}

fn watched_scene(ui: &mut Ui) {
    scene(ui);
    ui.watch_pointer(PointerWake::BUTTONS);
}

/// What one frame's pass A saw of the left button: the target's state, and the `Up`s watchers got.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LeftFrame {
    state: ButtonState,
    ups: usize,
}

fn step_watched_until_idle(h: &mut UiHarness) -> Vec<LeftFrame> {
    sample_until_idle(h, watched_scene, |ui| LeftFrame {
        state: ui.response_for(target()).left,
        ups: ui
            .pointer_events()
            .iter()
            .filter(|e| matches!(e, PointerEvent::Up { .. }))
            .count(),
    })
}

/// `[P, R, P]` fed together lands one change a frame, each edge once: the second press finds the
/// first already released by the time it applies, so no release is made up for it.
#[test]
fn a_fast_double_press_reports_each_edge_once() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, watched_scene);
    h.press_at(TARGET);
    h.release();
    h.press_at(TARGET);
    let frames = step_watched_until_idle(&mut h);
    let states: Vec<ButtonState> = frames.iter().map(|f| f.state).collect();
    assert_eq!(states, [down(1), up_click(1), down(2)]);
    assert_eq!(frames.iter().map(|f| f.ups).sum::<usize>(), 1, "{frames:?}");
}

/// A press while the button is still down means its release was lost: that capture ends as a cut
/// gesture, with no click, in its own frame, and the new press lands in the next.
#[test]
fn a_press_of_a_held_button_ends_the_lost_capture_without_a_click() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, watched_scene);
    h.press_at(TARGET);
    h.step(watched_scene);
    h.ui.on_input(
        InputEvent::PointerPressed(PointerButton::Left),
        Duration::ZERO,
    );
    let frames = step_watched_until_idle(&mut h);
    let states: Vec<ButtonState> = frames.iter().map(|f| f.state).collect();
    assert_eq!(
        states,
        [
            ButtonState::new(ButtonPhase::Up { click: None }, Drag::None),
            down(2),
        ],
    );
    assert_eq!(
        frames.iter().map(|f| f.ups).sum::<usize>(),
        0,
        "nothing was released"
    );
}

/// Losing surface focus right after a press waits a frame, so the press is seen before focus loss
/// ends it.
#[test]
fn focus_loss_after_a_press_ends_it_the_frame_after() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.press_at(TARGET);
    h.on_input(InputEvent::SurfaceFocusLost);
    assert_eq!(
        step_until_idle(&mut h),
        [
            down(1),
            ButtonState::new(ButtonPhase::Up { click: None }, Drag::None),
        ],
    );
}

fn replacement() -> WidgetId {
    WidgetId::from_hash("replacement")
}

/// The scene with the target gone and another drag target in its place.
fn replaced_scene(ui: &mut Ui) {
    Panel::hstack().auto_id().show(ui, |ui| {
        Panel::zstack()
            .id(replacement())
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::DRAG)
            .show(ui, |_| {});
    });
}

/// An eviction's stop edge lands in the frame after its widget vanished; a press held behind it
/// waits one more, so a stop and a fresh press of one button never share a frame.
#[test]
fn an_evicted_capture_and_a_held_press_never_share_a_frame() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.prime(2, scene);
    h.press_at(TARGET);
    h.drag_to(TARGET + Vec2::new(DRAG_THRESHOLD * 4.0, 0.0));
    h.release();
    h.press_at(TARGET);
    let frames = sample_until_idle(&mut h, replaced_scene, |ui| {
        ui.pointer_actions()
            .filter(|action| action.button == PointerButton::Left)
            .map(|action| action.edge)
            .collect::<Vec<_>>()
    });
    assert!(
        frames
            .iter()
            .any(|edges| edges.contains(&PointerEdge::DragStopped)),
        "the eviction stops the drag: {frames:?}",
    );
    for edges in &frames {
        assert!(
            !(edges.contains(&PointerEdge::DragStopped)
                && edges
                    .iter()
                    .any(|e| matches!(e, PointerEdge::Pressed { .. }))),
            "a stop and a press share a frame: {frames:?}",
        );
    }
    assert!(
        frames
            .iter()
            .flatten()
            .any(|e| matches!(e, PointerEdge::Pressed { .. })),
        "the held press lands: {frames:?}",
    );
}

/// A capture whose widget vanishes mid-drag ends with a stop edge; its frame records rather than paints from the retained tree.
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
