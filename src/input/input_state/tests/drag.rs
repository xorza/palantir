use crate::Ui;
use crate::input::capture::DRAG_THRESHOLD;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::response::Response;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

fn build_clickable(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .sense(Sense::CLICK)
        .show(ui, |_| {});
}

fn build_draggable(ui: &mut Ui) {
    // Wider sense so press routing accepts non-left buttons. `clicks()`
    // is true for both CLICK and DRAG, so this still captures.
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .sense(Sense::DRAG)
        .show(ui, |_| {});
}

fn id() -> WidgetId {
    WidgetId::from_hash("target")
}

#[test]
fn drag_delta_none_before_press() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.move_to(Vec2::new(50.0, 50.0));
    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        None,
        "no press → no drag",
    );
}

#[test]
fn drag_delta_tracks_pointer_minus_press() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(20.0, 30.0));
    h.drag_to(Vec2::new(80.0, 70.0));

    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        Some(Vec2::new(60.0, 40.0)),
        "delta = current - press_pos",
    );
}

#[test]
fn drag_delta_persists_when_pointer_leaves_widget_rect() {
    let s = UVec2::new(400, 400);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(50.0, 50.0));
    h.drag_to(Vec2::new(300.0, 200.0));

    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        Some(Vec2::new(250.0, 150.0)),
    );
}

#[test]
fn held_is_rect_independent_unlike_pressed() {
    // `held` reports "the left press is latched on this widget" regardless
    // of where the pointer has moved — unlike `pressed`, which also demands
    // the pointer stay over the widget. This is the signal drag-select
    // rides so it keeps tracking after the pointer leaves the editor.
    let s = UVec2::new(400, 400);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);

    // Idle over the widget: neither pressed nor held.
    h.move_to(Vec2::new(50.0, 50.0));
    let r = h.response_in(id(), build_clickable);
    assert!(
        !r.left.held() && !r.pressed(),
        "hover without press is neither"
    );

    // Press inside: both live, pointer is over the widget.
    h.press();
    let r = h.response_in(id(), build_clickable);
    assert!(
        r.left.held() && r.pressed(),
        "press over the widget sets both"
    );

    // Drag well outside the 100×100 rect: `pressed` drops (no longer
    // hovered), `held` stays — the capture is still latched.
    h.drag_to(Vec2::new(300.0, 300.0));
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.held(), "held survives the pointer leaving the rect");
    assert!(
        !r.pressed(),
        "pressed dies once the pointer leaves the rect"
    );

    // Release ends the capture: held clears.
    h.release();
    let r = h.response_in(id(), build_clickable);
    assert!(!r.left.held() && !r.pressed(), "release clears the capture");
}

#[test]
fn drag_delta_clears_on_release() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(30.0, 30.0));
    h.drag_to(Vec2::new(70.0, 70.0));
    assert!(
        h.response_in(id(), build_clickable)
            .left
            .drag
            .delta()
            .is_some()
    );

    h.release();
    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        None,
        "release ends the drag (active cleared)",
    );
}

/// Leaving the surface mid-drag is the gesture working, not ending: the
/// capture stays latched, so the drag keeps reporting the travel it had
/// and no stop edge fires. A commit-on-release gesture must not commit on
/// a window-exit — that would split one scrub into two undo entries.
///
/// The travel is read off the press rather than the live pointer, which
/// is what lets it survive a pointer that is `None`. Denying the drag
/// here would also put this reader at odds with `pointer_actions`, which
/// reads the latch and would go on reporting the same drag.
#[test]
fn a_drag_survives_the_pointer_leaving_the_surface() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(90.0, 40.0));
    h.pointer_left();

    // 90 - 40 = 50 px of travel, held across the leave.
    let r = h.response_in(id(), build_clickable);
    assert_eq!(r.left.drag.delta(), Some(Vec2::new(50.0, 0.0)));
    assert!(r.left.drag.dragging(), "the capture is still latched");
    assert!(
        !r.left.drag.stopped(),
        "pointer-left is not a release; the stop edge must wait for it",
    );

    // Re-enter with the button still held: the same drag resumes
    // (no new start edge), and the real release fires the stop edge.
    h.move_to(Vec2::new(100.0, 40.0));
    let r = h.response_in(id(), build_clickable);
    assert_eq!(r.left.drag.delta(), Some(Vec2::new(60.0, 0.0)));
    assert!(!r.left.drag.started(), "re-entry resumes, not re-latches");

    h.release();
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped());
}

#[test]
fn drag_stopped_edge_fires_once_on_release() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(30.0, 30.0));
    h.drag_to(Vec2::new(70.0, 30.0));

    // Mid-drag: no stop edge, drag observable.
    let r = h.response_in(id(), build_draggable);
    assert!(r.middle.drag.dragging() && !r.middle.drag.stopped());

    // Release frame: the drag itself is gone, only the edge remains,
    // and it carries the button.
    h.release_button(PointerButton::Middle);
    let r = h.response_in(id(), build_draggable);
    assert!(!r.middle.drag.dragging(), "release destroys the drag state");
    assert!(r.middle.drag.stopped());
    assert!(!r.left.drag.stopped(), "edge is button-filtered",);

    // One-frame edge: gone the next frame.
    let r = h.response_in(id(), build_draggable);
    assert!(!r.middle.drag.stopped());
}

#[test]
fn sub_threshold_release_fires_click_not_drag_stopped() {
    // A press+release without crossing DRAG_THRESHOLD is a click; no
    // drag ever latched, so no stop edge may fire.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(50.0, 50.0));
    h.move_to(Vec2::new(51.0, 50.0));
    h.release();

    let r = h.response_in(id(), build_clickable);
    assert!(r.left.clicked(), "sub-threshold press+release is a click");
    assert!(!r.left.drag.stopped(), "no drag latched, no stop edge");
}

#[test]
fn drag_delta_only_for_active_widget() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(20.0, 20.0));
    h.drag_to(Vec2::new(60.0, 50.0));

    let other = WidgetId::from_hash("other");
    assert_eq!(
        h.response_in(other, build_clickable).left.drag.delta(),
        None,
        "only the captured widget sees the drag delta",
    );
}

#[test]
fn middle_drag_tracks_pointer_minus_press_after_latch() {
    // Middle-button press anchors at (20, 30); pointer moves to
    // (80, 70). Travel = sqrt(60^2 + 40^2) = 72.1 px > DRAG_THRESHOLD
    // (4 px) so the drag latches.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(20.0, 30.0));
    h.drag_to(Vec2::new(80.0, 70.0));

    let r = h.response_in(id(), build_draggable);
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)));
    assert!(
        r.middle.drag.started(),
        "drag-start edge must fire on the threshold-crossing move",
    );
    assert!(r.middle.drag.dragging());
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)));
    assert!(r.middle.drag.started());
}

#[test]
fn middle_drag_does_not_expose_delta_below_threshold() {
    // Press + 3 px wiggle = no latch. `started` stays false and
    // `delta` is `None`, mirroring left-button semantics.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(50.0, 50.0));
    h.move_to(Vec2::new(52.0, 51.0));

    let r = h.response_in(id(), build_draggable);
    assert_eq!(r.middle.drag.delta(), None);
    assert!(!r.middle.drag.started());
    assert!(!r.middle.drag.dragging());
}

#[test]
fn drag_started_is_one_frame_edge_then_clears_on_post_record() {
    // The `started` flag is a single-frame edge: true on the frame that
    // observes the latching move, false on the next frame even while the
    // drag continues. Each `resp` runs one frame, so the first observes
    // the edge (and its `post_record` clears it) and the second sees it
    // gone.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(50.0, 50.0));
    h.drag_to(Vec2::new(80.0, 50.0)); // latches
    assert!(h.response_in(id(), build_draggable).middle.drag.started());

    h.drag_to(Vec2::new(100.0, 50.0));
    let r = h.response_in(id(), build_draggable);
    assert!(
        !r.middle.drag.started(),
        "started must clear after one frame",
    );
    assert_eq!(
        r.middle.drag.delta(),
        Some(Vec2::new(50.0, 0.0)),
        "delta keeps tracking",
    );
}

#[test]
fn right_button_drag_also_latches() {
    // The drag-latch loop iterates every PointerButton, so right
    // drag works the same as left/middle.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Right, Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(70.0, 40.0));

    let r = h.response_in(id(), build_draggable);
    assert_eq!(r.right.drag.delta(), Some(Vec2::new(30.0, 0.0)));
    assert!(r.right.drag.started());
}

#[test]
fn left_wins_over_simultaneously_latched_middle() {
    // Both left and middle are latched on the same widget. Only one
    // drag is reported — the priority-first in `PointerButton::all()`
    // (left). `dragged_by(Middle)` is false even though the middle
    // press is still captured.
    let s = UVec2::new(300, 300);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_at(Vec2::new(20.0, 20.0));
    h.drag_to(Vec2::new(40.0, 20.0)); // latches left
    h.press_button(PointerButton::Middle);
    h.drag_to(Vec2::new(100.0, 60.0)); // latches middle

    let r = h.response_in(id(), build_draggable);
    let d = r.left.drag.delta().expect("a drag must be active");
    assert!(
        !r.middle.drag.dragging(),
        "left has priority over middle — only one drag slot populates"
    );
    // Left was pressed at (20, 20); current pointer (100, 60).
    assert_eq!(d, Vec2::new(80.0, 40.0));
    assert!(r.left.drag.dragging());
    assert!(
        !r.middle.drag.dragging(),
        "middle is captured but not the active drag",
    );
}

#[test]
fn releasing_priority_button_promotes_lower_priority() {
    // After releasing left while middle is still held + latched, the
    // active drag transitions to middle without the user lifting
    // anything else.
    let s = UVec2::new(300, 300);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_at(Vec2::new(20.0, 20.0));
    h.press_button(PointerButton::Middle);
    h.drag_to(Vec2::new(80.0, 60.0)); // both latch

    assert!(h.response_in(id(), build_draggable).left.drag.dragging());

    h.release();
    let r = h.response_in(id(), build_draggable);
    assert!(
        r.middle.drag.dragging(),
        "releasing left must promote middle to the active drag",
    );
    assert!(!r.left.drag.dragging());
    // Middle's anchor is the middle press position (same frame as
    // left's, so (20, 20)); delta = current (80, 60) - press (20, 20).
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)),);
}

#[test]
fn drag_zero_state_for_uncaptured_widget() {
    // A widget that didn't capture the press sees the zero state
    // regardless of which button is being dragged elsewhere.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(50.0, 50.0));
    h.drag_to(Vec2::new(80.0, 70.0));

    let other = WidgetId::from_hash("other");
    let r = h.response_in(other, build_draggable);
    assert_eq!(r.middle.drag.delta(), None);
    assert!(!r.middle.drag.dragging());
    assert!(!r.middle.drag.started());
}

#[test]
fn drag_delta_none_when_press_missed_all_widgets() {
    // Outer non-clickable wraps a small clickable so the root doesn't
    // auto-fill the surface and swallow the press.
    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("target"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui, |_| {});
        });
    };
    let mut h = UiHarness::new(surface);
    h.frame(build);
    h.press_at(Vec2::new(200.0, 200.0));
    h.drag_to(Vec2::new(250.0, 220.0));
    assert_eq!(h.response_in(id(), build).left.drag.delta(), None,);
}

// Drag-on-canvas composition, driven through the widget-facing
// `Response` API: callers snapshot an `anchor` on `r.drag_started()`
// and compose `pos = anchor + r.drag_delta()` each frame. `Ui::frame`
// re-records on action input, so the dragged position lands in the
// same frame as the move event. The `Card` fixture drives that
// pattern end-to-end: threshold latch, position tracking,
// click-suppression-after-drag, multi-widget isolation.
const CARD_SIZE: f32 = 60.0;
const SURFACE: UVec2 = UVec2::new(400, 400);

fn card_id(label: &str) -> WidgetId {
    WidgetId::from_hash(label)
}

#[derive(Debug)]
struct Card {
    label: &'static str,
    pos: Vec2,
    anchor: Vec2,
    clicked: bool,
}

impl Card {
    fn new(label: &'static str, pos: Vec2) -> Self {
        Self {
            label,
            pos,
            anchor: pos,
            clicked: false,
        }
    }

    fn record(&mut self, ui: &mut Ui) {
        let r = Frame::new()
            .id(WidgetId::from_hash(self.label))
            .size((Sizing::fixed(CARD_SIZE), Sizing::fixed(CARD_SIZE)))
            .position(self.pos)
            .sense(Sense::DRAG)
            .show(ui);
        self.fold(&r);
    }

    // Idempotent across the multi-pass `Ui::frame` rebuild — pass 2
    // would otherwise overwrite the click with `false` and miss the
    // one-shot drag_started.
    fn fold(&mut self, r: &Response) {
        if r.left.drag.started() {
            self.anchor = self.pos;
        }
        if let Some(delta) = r.left.drag.delta() {
            self.pos = self.anchor + delta;
        }
        self.clicked |= r.left.clicked();
    }
}

fn frame_with(h: &mut UiHarness, mut body: impl FnMut(&mut Ui)) {
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("canvas"))
                .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                .show(ui, |ui| body(ui));
        });
    });
}

#[test]
fn sub_threshold_keeps_position_and_emits_click() {
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    h.press_at(press);
    h.move_to(press + Vec2::new(2.0, 2.0));
    h.release();

    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(
        a.pos,
        Vec2::new(50.0, 50.0),
        "sub-threshold leaves position"
    );
    assert!(a.clicked, "sub-threshold gesture still fires click");
}

#[test]
fn supra_threshold_moves_widget_and_suppresses_click() {
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    let drop = press + Vec2::new(40.0, 0.0);
    h.press_at(press);
    h.move_to(drop);

    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(
        a.pos,
        Vec2::new(90.0, 50.0),
        "position = anchor + delta on latch frame"
    );
    assert!(!a.clicked, "click does not fire mid-drag");

    h.release();
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(90.0, 50.0), "release re-grounds position");
    assert!(!a.clicked, "drag suppresses release-click");
}

#[test]
fn drag_then_release_then_drag_restarts_from_new_anchor() {
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    h.press_at(Vec2::new(80.0, 80.0));
    h.drag_to(Vec2::new(110.0, 80.0));
    frame_with(&mut h, |ui| a.record(ui));
    h.release();
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(80.0, 50.0));

    h.press_at(Vec2::new(100.0, 70.0));
    h.drag_to(Vec2::new(120.0, 80.0));
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(100.0, 60.0), "second drag composes");
}

#[test]
fn only_pressed_card_moves_in_two_card_scene() {
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(20.0, 20.0));
    let mut b = Card::new("b", Vec2::new(200.0, 20.0));

    frame_with(&mut h, |ui| {
        a.record(ui);
        b.record(ui);
    });

    h.press_at(Vec2::new(220.0, 40.0));
    h.drag_to(Vec2::new(260.0, 40.0));

    frame_with(&mut h, |ui| {
        a.record(ui);
        b.record(ui);
    });

    assert_eq!(a.pos, Vec2::new(20.0, 20.0), "card A undisturbed");
    assert_eq!(b.pos, Vec2::new(240.0, 20.0), "card B moves by drag delta");
}

#[test]
fn drag_started_fires_only_on_latch_frame() {
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    let mut started = vec![];

    let mut step = |h: &mut UiHarness, a: &mut Card| {
        let mut latched = false;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("canvas"))
                    .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                    .show(ui, |ui| {
                        a.record(ui);
                        latched |= ui.response_for(card_id("a")).left.drag.started();
                    });
            });
        });
        started.push(latched);
    };

    step(&mut h, &mut a);
    h.press_at(Vec2::new(80.0, 80.0));
    step(&mut h, &mut a);
    h.move_to(Vec2::new(82.0, 81.0));
    step(&mut h, &mut a);
    let supra = Vec2::new(80.0 + DRAG_THRESHOLD + 1.0, 80.0);
    h.move_to(supra);
    step(&mut h, &mut a);
    h.move_to(supra + Vec2::new(10.0, 0.0));
    step(&mut h, &mut a);

    assert_eq!(
        started,
        vec![false, false, false, true, false],
        "drag_started fires exactly on the latch frame"
    );
}

#[test]
fn canvas_rearranges_with_dragged_child_position() {
    // `Ui::frame` re-records on action input, so pass-2 picks up the
    // dragged position and the same-frame layout reflects it.
    let mut h = UiHarness::new(SURFACE);
    let mut a = Card::new("a", Vec2::new(40.0, 40.0));
    frame_with(&mut h, |ui| a.record(ui));

    h.press_at(Vec2::new(60.0, 60.0));
    h.drag_to(Vec2::new(150.0, 60.0));

    let mut card_node = None;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("canvas"))
                .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                .show(ui, |ui| {
                    let r = Frame::new()
                        .id(WidgetId::from_hash("a"))
                        .size((Sizing::fixed(CARD_SIZE), Sizing::fixed(CARD_SIZE)))
                        .position(a.pos)
                        .sense(Sense::DRAG)
                        .show(ui);
                    card_node = Some(r.node());
                    a.fold(&r);
                });
        });
    });

    let rect = h.ui.arranged_rect(Layer::Main, card_node.unwrap());
    assert!(
        (rect.min.x - 130.0).abs() < 0.5,
        "drag lands within the frame: anchor(40) + delta(90) = 130, got {}",
        rect.min.x,
    );
    assert!(
        (a.pos.x - 130.0).abs() < 0.5,
        "pos = anchor(40) + delta(90)"
    );
}

/// A capture evicted because its widget left the tree still ends through
/// a release edge, so the gesture finishes for everyone reading it.
///
/// Dropping the press on its own ends it for the state machine alone: no
/// `Drag::Stopped`, no `ButtonPhase::Up`, no `PointerEdge::DragStopped`,
/// and the later real release finds nothing left to report. `Slider` and
/// `DragValue` commit on `drag.stopped()`, so a widget that skips one
/// frame mid-drag silently loses the commit.
#[test]
fn a_capture_evicted_mid_drag_still_ends_with_its_stop_edge() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.press_at(Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(90.0, 40.0));
    assert!(
        h.response_in(id(), build_clickable).left.drag.dragging(),
        "the drag is live before the widget goes away",
    );

    // The widget skips a frame. `end_frame` evicts the capture.
    h.frame(|_| {});

    // It comes back, and reads the edge its gesture owed it.
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped(), "eviction owes the stop edge");
    assert!(!r.left.drag.dragging(), "and the drag itself is over");
    assert_eq!(
        r.left.click_count(),
        0,
        "a widget that vanished was not clicked",
    );
}

/// A sub-threshold press evicted the same way dissolves without claiming
/// a click — nothing landed on a widget that is not there.
#[test]
fn a_capture_evicted_before_the_drag_threshold_reports_no_click() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.press_at(Vec2::new(40.0, 40.0));
    h.frame(|_| {});

    let r = h.response_in(id(), build_clickable);
    assert_eq!(r.left.click_count(), 0, "no click without a release on it");
    assert!(!r.left.drag.stopped(), "and no drag to stop");
    assert!(!r.left.held(), "the capture is gone");
}

/// Losing surface focus ends every gesture and forgets the modifiers.
///
/// The platform stops reporting to an unfocused surface, so the release
/// and the modifier drop that happen over there are never seen. Without
/// this the press stays latched and the first click back into the window
/// completes a gesture the user abandoned.
#[test]
fn surface_focus_loss_ends_every_capture_and_clears_modifiers() {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::modifiers::Modifiers;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.on_input(InputEvent::ModifiersChanged(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }));
    h.press_at(Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(90.0, 40.0));

    h.on_input(InputEvent::SurfaceFocusLost);
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped(), "the drag gets its commit edge");
    assert!(!r.left.held(), "and the press is no longer latched");
    assert_eq!(
        h.ui.input().modifiers,
        Modifiers::default(),
        "a modifier held into another window is not held here",
    );
}
