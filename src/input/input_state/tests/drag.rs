use crate::Ui;
use crate::input::capture::DRAG_THRESHOLD;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widgets::{block::Block, panel::Panel};
use glam::{UVec2, Vec2};

fn build_clickable(ui: &mut Ui) {
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .sense(Sense::CLICK)
        .show(ui, |_| {});
}

fn build_draggable(ui: &mut Ui) {
    // Wider sense so press routing accepts non-left buttons; `clicks()` is true for
    // CLICK and DRAG.
    Panel::hstack()
        .id(WidgetId::from_hash("target"))
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .sense(Sense::DRAG)
        .show(ui, |_| {});
}

fn id() -> WidgetId {
    WidgetId::from_hash("target")
}

/// The drag target beside a second draggable the press does not land on, for
/// isolation tests.
fn build_target_and_bystander(ui: &mut Ui) {
    Panel::hstack().auto_id().show(ui, |ui| {
        for name in ["target", "other"] {
            Panel::hstack()
                .id(WidgetId::from_hash(name))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .sense(Sense::DRAG)
                .show(ui, |_| {});
        }
    });
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
    // `held` is "the left press is latched on this widget" wherever the pointer has
    // gone, unlike `pressed`; drag-select rides it to keep tracking after the
    // pointer leaves the editor.
    let s = UVec2::new(400, 400);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);

    h.move_to(Vec2::new(50.0, 50.0));
    let r = h.response_in(id(), build_clickable);
    assert!(
        !r.left.held() && !r.pressed(),
        "hover without press is neither"
    );

    h.press();
    let r = h.response_in(id(), build_clickable);
    assert!(
        r.left.held() && r.pressed(),
        "press over the widget sets both"
    );

    // Drag well outside the 100×100 rect: `pressed` drops, `held` stays.
    h.drag_to(Vec2::new(300.0, 300.0));
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.held(), "held survives the pointer leaving the rect");
    assert!(
        !r.pressed(),
        "pressed dies once the pointer leaves the rect"
    );

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
    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        Some(Vec2::new(40.0, 40.0)),
        "press (30, 30) → drag (70, 70): 40 px of travel on each axis",
    );

    h.release();
    assert_eq!(
        h.response_in(id(), build_clickable).left.drag.delta(),
        None,
        "release ends the drag (active cleared)",
    );
}

/// Leaving the surface mid-drag is the gesture working, not ending: the capture
/// stays latched, the drag keeps reporting its travel and no stop edge fires, so a
/// commit-on-release gesture does not split one scrub into two undo entries. The
/// travel is read off the press, not the live pointer, so it survives a `None`
/// pointer (and agrees with `pointer_actions`, which reads the latch).
#[test]
fn a_drag_survives_the_pointer_leaving_the_surface() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_clickable);
    h.press_at(Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(90.0, 40.0));
    h.pointer_left();

    let r = h.response_in(id(), build_clickable);
    assert_eq!(r.left.drag.delta(), Some(Vec2::new(50.0, 0.0)));
    assert!(r.left.drag.is_live(), "the capture is still latched");
    assert!(
        !r.left.drag.stopped(),
        "pointer-left is not a release; the stop edge must wait for it",
    );

    // Re-entering with the button held resumes the same drag; the real release
    // fires the stop edge.
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

    let r = h.response_in(id(), build_draggable);
    assert!(r.middle.drag.is_live() && !r.middle.drag.stopped());

    h.release_button(PointerButton::Middle);
    let r = h.response_in(id(), build_draggable);
    assert!(!r.middle.drag.is_live(), "release destroys the drag state");
    assert!(r.middle.drag.stopped());
    assert!(!r.left.drag.stopped(), "edge is button-filtered");

    let r = h.response_in(id(), build_draggable);
    assert!(!r.middle.drag.stopped());
}

#[test]
fn sub_threshold_release_fires_click_not_drag_stopped() {
    // A press+release without crossing DRAG_THRESHOLD is a click: no drag latched,
    // no stop edge.
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
    h.frame(build_target_and_bystander);
    h.press_at(Vec2::new(20.0, 20.0));
    h.drag_to(Vec2::new(60.0, 50.0));

    let [target, other] = h.frame_value(|ui| {
        build_target_and_bystander(ui);
        [id(), WidgetId::from_hash("other")].map(|w| ui.response_for(w).left.drag.delta())
    });
    assert_eq!(target, Some(Vec2::new(40.0, 30.0)), "the captured widget");
    assert_eq!(other, None, "only the captured widget sees the drag delta");
}

#[test]
fn middle_drag_tracks_pointer_minus_press_after_latch() {
    // Middle press at (20, 30), pointer to (80, 70): travel 72.1 px >
    // DRAG_THRESHOLD (4 px).
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
    assert!(r.middle.drag.is_live());
}

#[test]
fn middle_drag_does_not_expose_delta_below_threshold() {
    // Press + 3 px wiggle = no latch: `started` false, `delta` `None`.
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_button_at(PointerButton::Middle, Vec2::new(50.0, 50.0));
    h.move_to(Vec2::new(52.0, 51.0));

    let r = h.response_in(id(), build_draggable);
    assert_eq!(r.middle.drag.delta(), None);
    assert!(!r.middle.drag.started());
    assert!(!r.middle.drag.is_live());
}

#[test]
fn drag_started_is_one_frame_edge_then_clears_on_the_next_frame() {
    // `started` is a single-frame edge: true on the frame observing the latching
    // move, false the next.
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
    // Left and middle are both latched; only the priority-first in
    // `PointerButton::ALL` (left) is reported.
    let s = UVec2::new(300, 300);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_at(Vec2::new(20.0, 20.0));
    h.drag_to(Vec2::new(40.0, 20.0)); // latches left
    h.press_button(PointerButton::Middle);
    h.drag_to(Vec2::new(100.0, 60.0)); // latches middle

    let r = h.response_in(id(), build_draggable);
    let d = r.left.drag.delta().expect("a drag must be active");
    assert_eq!(d, Vec2::new(80.0, 40.0));
    assert!(r.left.drag.is_live());
    assert!(
        !r.middle.drag.is_live(),
        "left has priority: middle is captured but not the active drag",
    );
}

#[test]
fn releasing_priority_button_promotes_lower_priority() {
    // Releasing left while middle is held and latched moves the active drag to
    // middle.
    let s = UVec2::new(300, 300);
    let mut h = UiHarness::new(s);
    h.frame(build_draggable);
    h.press_at(Vec2::new(20.0, 20.0));
    h.press_button(PointerButton::Middle);
    h.drag_to(Vec2::new(80.0, 60.0)); // both latch

    assert!(h.response_in(id(), build_draggable).left.drag.is_live());

    h.release();
    let r = h.response_in(id(), build_draggable);
    assert!(
        r.middle.drag.is_live(),
        "releasing left must promote middle to the active drag",
    );
    assert!(!r.left.drag.is_live());
    // Middle's anchor is its own press position (20, 20); delta = current (80, 60)
    // - press.
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)));
}

#[test]
fn drag_zero_state_for_uncaptured_widget() {
    let s = UVec2::new(200, 200);
    let mut h = UiHarness::new(s);
    h.frame(build_target_and_bystander);
    h.press_button_at(PointerButton::Middle, Vec2::new(50.0, 50.0));
    h.drag_to(Vec2::new(80.0, 70.0));

    let [target, other] = h.frame_value(|ui| {
        build_target_and_bystander(ui);
        [id(), WidgetId::from_hash("other")].map(|w| ui.response_for(w).middle.drag)
    });
    assert!(target.started(), "control: the captured widget latched");
    assert_eq!(other.delta(), None);
    assert!(!other.is_live());
    assert!(!other.started());
}

#[test]
fn drag_delta_none_when_press_missed_all_widgets() {
    // The outer non-clickable wraps a small clickable so the root does not
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
    assert_eq!(h.response_in(id(), build).left.drag.delta(), None);
}

// Drag-on-canvas composition through the widget-facing `Response` API: callers
// snapshot an `anchor` on `r.drag_started()` and compose `pos = anchor +
// r.drag_delta()`. The `Card` fixture drives threshold latch, position tracking,
// click-suppression-after-drag and multi-widget isolation.
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
    /// Clicks seen across every pass of every frame, so a click reported by both
    /// passes reads as a double fire.
    clicks: u32,
}

impl Card {
    fn new(label: &'static str, pos: Vec2) -> Self {
        Self {
            label,
            pos,
            anchor: pos,
            clicks: 0,
        }
    }

    fn record(&mut self, ui: &mut Ui) {
        let r = Block::new()
            .id(WidgetId::from_hash(self.label))
            .size((Sizing::fixed(CARD_SIZE), Sizing::fixed(CARD_SIZE)))
            .position(self.pos)
            .sense(Sense::DRAG)
            .show(ui);
        self.fold(&r);
    }

    // Runs on every pass as an app's handler does: pass B sees the edges drained.
    fn fold(&mut self, r: &Response<'_>) {
        if r.left.drag.started() {
            self.anchor = self.pos;
        }
        if let Some(delta) = r.left.drag.delta() {
            self.pos = self.anchor + delta;
        }
        self.clicks += u32::from(r.left.clicked());
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
    assert_eq!(a.clicks, 1, "sub-threshold gesture still fires one click");
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
    assert_eq!(a.clicks, 0, "click does not fire mid-drag");

    h.release();
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(90.0, 50.0), "release re-grounds position");
    assert_eq!(a.clicks, 0, "drag suppresses release-click");
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
        let latches = h
            .frame_passes(|ui| {
                Panel::hstack()
                    .auto_id()
                    .show(ui, |ui| {
                        Panel::canvas()
                            .id(WidgetId::from_hash("canvas"))
                            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                            .show(ui, |ui| {
                                a.record(ui);
                                ui.response_for(card_id("a")).left.drag.started()
                            })
                            .inner
                    })
                    .inner
            })
            .count_where(|&started| started);
        started.push(latches);
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
        vec![0, 0, 0, 1, 0],
        "drag_started fires on the latch frame, in one pass"
    );
}

#[test]
fn canvas_rearranges_with_dragged_child_position() {
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
                    let r = Block::new()
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
    assert_eq!(
        rect.min.x, 130.0,
        "drag lands within the frame: anchor(40) + delta(90) = 130",
    );
    assert_eq!(a.pos.x, 130.0, "pos = anchor(40) + delta(90)");
}

/// A capture evicted because its widget left the tree still ends through a release
/// edge. Dropping the press alone would end it for the state machine only, so
/// `Slider` and `DragValue`, which commit on `drag.stopped()`, would lose the
/// commit when a widget skips a frame mid-drag.
#[test]
fn a_capture_evicted_mid_drag_still_ends_with_its_stop_edge() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.press_at(Vec2::new(40.0, 40.0));
    h.drag_to(Vec2::new(90.0, 40.0));
    assert!(
        h.response_in(id(), build_clickable).left.drag.is_live(),
        "the drag is live before the widget goes away",
    );

    h.frame(|_| {});

    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped(), "eviction owes the stop edge");
    assert!(!r.left.drag.is_live(), "and the drag itself is over");
    assert_eq!(
        r.left.click_count(),
        0,
        "a widget that vanished was not clicked",
    );
}

/// A sub-threshold press evicted the same way dissolves without claiming a click.
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

/// Losing surface focus ends every gesture and forgets the modifiers: an unfocused
/// surface never reports the release, so the press would stay latched.
#[test]
fn surface_focus_loss_ends_every_capture_and_clears_modifiers() {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::modifiers::Modifiers;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.on_input(InputEvent::ModifiersChanged(Modifiers::CTRL));
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
