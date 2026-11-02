use crate::host::harness::Harness;
use crate::input::input_event::InputEvent;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::input_state::capture::DRAG_THRESHOLD;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
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

/// Every button drags alike, on a draggable beside a second one the press misses:
/// - with no press, and after a press that moves 2.2 px, under the 4 px threshold, there is no
///   drag; that release is a click and no stop edge;
/// - a press at (20, 30) moved to (80, 70) latches: the delta is (60, 40), the start edge fires,
///   and the second draggable sees nothing;
/// - the next frame, at (300, 200) past the target's rect, the delta is (280, 170) and the start
///   edge is gone;
/// - the release ends the drag and fires the stop edge for that button alone, for one frame.
#[test]
fn each_button_latches_tracks_and_isolates_a_drag() {
    let other = WidgetId::from_hash("other");
    for button in PointerButton::ALL {
        let mut h = Harness::mono(UVec2::new(400, 400));
        h.frame(build_target_and_bystander);
        let drag = |h: &mut Harness| {
            h.response_in(id(), build_target_and_bystander)
                .button(button)
                .drag
        };

        h.hover(Vec2::new(50.0, 50.0));
        assert_eq!(drag(&mut h).delta(), None, "{button:?}: no press");

        h.press(Vec2::new(50.0, 50.0), button);
        h.hover(Vec2::new(52.0, 51.0));
        let wiggle = drag(&mut h);
        assert_eq!(wiggle.delta(), None, "{button:?}: under the threshold");
        assert!(!wiggle.started() && !wiggle.is_live(), "{button:?}");
        h.release(button);
        let r = *h
            .response_in(id(), build_target_and_bystander)
            .button(button);
        assert!(r.clicked(), "{button:?}: the short gesture is a click");
        assert!(
            !r.drag.stopped(),
            "{button:?}: no drag latched, no stop edge"
        );
        h.advance_past_double_click();

        h.press(Vec2::new(20.0, 30.0), button);
        h.drag_to(Vec2::new(80.0, 70.0));
        let [target, bystander] = h.frame_value(|ui| {
            build_target_and_bystander(ui);
            [id(), other].map(|w| ui.response_for(w).button(button).drag)
        });
        assert_eq!(target.delta(), Some(Vec2::new(60.0, 40.0)), "{button:?}");
        assert!(target.started() && target.is_live(), "{button:?}: latched");
        assert_eq!(bystander.delta(), None, "{button:?}: the bystander");
        assert!(!bystander.started() && !bystander.is_live(), "{button:?}");

        h.drag_to(Vec2::new(300.0, 200.0));
        let moved = drag(&mut h);
        assert_eq!(moved.delta(), Some(Vec2::new(280.0, 170.0)), "{button:?}");
        assert!(
            !moved.started(),
            "{button:?}: the start edge lasts one frame"
        );

        h.release(button);
        let r = h.response_in(id(), build_target_and_bystander);
        assert!(r.button(button).drag.stopped(), "{button:?}: the stop edge");
        assert!(!r.button(button).drag.is_live(), "{button:?}");
        assert_eq!(r.button(button).drag.delta(), None, "{button:?}");
        for unpressed in PointerButton::ALL.into_iter().filter(|&b| b != button) {
            assert!(
                !r.button(unpressed).drag.stopped(),
                "{button:?}: {unpressed:?}"
            );
        }
        assert!(
            !drag(&mut h).stopped(),
            "{button:?}: the stop edge lasts one frame"
        );
    }
}

#[test]
fn held_is_rect_independent_unlike_pressed() {
    // `held` is "the left press is latched on this widget" wherever the pointer has
    // gone, unlike `pressed`; drag-select rides it to keep tracking after the
    // pointer leaves the editor.
    let s = UVec2::new(400, 400);
    let mut h = Harness::mono(s);
    h.frame(build_clickable);

    h.hover(Vec2::new(50.0, 50.0));
    let r = h.response_in(id(), build_clickable);
    assert!(
        !r.left.held() && !r.pressed(),
        "hover without press is neither"
    );

    h.on_input(InputEvent::PointerPressed(PointerButton::Left));
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

    h.release(PointerButton::Left);
    let r = h.response_in(id(), build_clickable);
    assert!(!r.left.held() && !r.pressed(), "release clears the capture");
}

/// Leaving the surface mid-drag is the gesture working, not ending: the capture
/// stays latched, the drag keeps reporting its travel and no stop edge fires, so a
/// commit-on-release gesture does not split one scrub into two undo entries. The
/// travel is read off the press, not the live pointer, so it survives a `None`
/// pointer (and agrees with `pointer_actions`, which reads the latch).
#[test]
fn a_drag_survives_the_pointer_leaving_the_surface() {
    let s = UVec2::new(200, 200);
    let mut h = Harness::mono(s);
    h.frame(build_clickable);
    h.press(Vec2::new(40.0, 40.0), PointerButton::Left);
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
    h.hover(Vec2::new(100.0, 40.0));
    let r = h.response_in(id(), build_clickable);
    assert_eq!(r.left.drag.delta(), Some(Vec2::new(60.0, 0.0)));
    assert!(!r.left.drag.started(), "re-entry resumes, not re-latches");

    h.release(PointerButton::Left);
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped());
}

/// Each latched button reports its own drag: two latched together each measure from their own
/// press, and one's release leaves the other's drag as it was, with no second latch edge.
#[test]
fn each_latched_button_reports_its_own_drag() {
    let s = UVec2::new(300, 300);
    let mut h = Harness::mono(s);
    h.frame(build_draggable);
    h.press(Vec2::new(20.0, 20.0), PointerButton::Left);
    h.drag_to(Vec2::new(40.0, 20.0)); // latches left
    h.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    h.drag_to(Vec2::new(100.0, 60.0)); // latches middle

    let r = h.response_in(id(), build_draggable);
    // Left travels (100, 60) - (20, 20); middle (100, 60) - (40, 20).
    assert_eq!(r.left.drag.delta(), Some(Vec2::new(80.0, 40.0)));
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)));
    assert!(r.middle.drag.started(), "middle's own latch edge");

    h.release(PointerButton::Left);
    let r = h.response_in(id(), build_draggable);
    assert!(r.left.drag.stopped());
    assert!(r.middle.drag.is_live());
    assert!(!r.middle.drag.started(), "no second latch edge");
    assert_eq!(r.middle.drag.delta(), Some(Vec2::new(60.0, 40.0)));
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
    let mut h = Harness::mono(surface);
    h.frame(build);
    h.press(Vec2::new(200.0, 200.0), PointerButton::Left);
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
            .pos(self.pos)
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

fn frame_with(h: &mut Harness, mut body: impl FnMut(&mut Ui)) {
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
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    h.press(press, PointerButton::Left);
    h.hover(press + Vec2::new(2.0, 2.0));
    h.release(PointerButton::Left);

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
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    let drop = press + Vec2::new(40.0, 0.0);
    h.press(press, PointerButton::Left);
    h.hover(drop);

    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(
        a.pos,
        Vec2::new(90.0, 50.0),
        "position = anchor + delta on latch frame"
    );
    assert_eq!(a.clicks, 0, "click does not fire mid-drag");

    h.release(PointerButton::Left);
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(90.0, 50.0), "release re-grounds position");
    assert_eq!(a.clicks, 0, "drag suppresses release-click");
}

#[test]
fn drag_then_release_then_drag_restarts_from_new_anchor() {
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut h, |ui| a.record(ui));

    h.press(Vec2::new(80.0, 80.0), PointerButton::Left);
    h.drag_to(Vec2::new(110.0, 80.0));
    frame_with(&mut h, |ui| a.record(ui));
    h.release(PointerButton::Left);
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(80.0, 50.0));

    h.press(Vec2::new(100.0, 70.0), PointerButton::Left);
    h.drag_to(Vec2::new(120.0, 80.0));
    frame_with(&mut h, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(100.0, 60.0), "second drag composes");
}

#[test]
fn only_pressed_card_moves_in_two_card_scene() {
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(20.0, 20.0));
    let mut b = Card::new("b", Vec2::new(200.0, 20.0));

    frame_with(&mut h, |ui| {
        a.record(ui);
        b.record(ui);
    });

    h.press(Vec2::new(220.0, 40.0), PointerButton::Left);
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
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    let mut started = vec![];

    let mut step = |h: &mut Harness, a: &mut Card| {
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
    h.press(Vec2::new(80.0, 80.0), PointerButton::Left);
    step(&mut h, &mut a);
    h.hover(Vec2::new(82.0, 81.0));
    step(&mut h, &mut a);
    let supra = Vec2::new(80.0 + DRAG_THRESHOLD + 1.0, 80.0);
    h.hover(supra);
    step(&mut h, &mut a);
    h.hover(supra + Vec2::new(10.0, 0.0));
    step(&mut h, &mut a);

    assert_eq!(
        started,
        vec![0, 0, 0, 1, 0],
        "drag_started fires on the latch frame, in one pass"
    );
}

#[test]
fn canvas_rearranges_with_dragged_child_position() {
    let mut h = Harness::mono(SURFACE);
    let mut a = Card::new("a", Vec2::new(40.0, 40.0));
    frame_with(&mut h, |ui| a.record(ui));

    h.press(Vec2::new(60.0, 60.0), PointerButton::Left);
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
                        .pos(a.pos)
                        .sense(Sense::DRAG)
                        .show(ui);
                    card_node = Some(r.node());
                    a.fold(&r);
                });
        });
    });

    let rect = h.ui().arranged_rect(Layer::Main, card_node.unwrap());
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
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.press(Vec2::new(40.0, 40.0), PointerButton::Left);
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
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.press(Vec2::new(40.0, 40.0), PointerButton::Left);
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
    use crate::input::keyboard::modifiers::Modifiers;

    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(build_clickable);
    h.on_input(InputEvent::ModifiersChanged(Modifiers::CTRL));
    h.press(Vec2::new(40.0, 40.0), PointerButton::Left);
    h.drag_to(Vec2::new(90.0, 40.0));

    h.on_input(InputEvent::SurfaceFocusLost);
    let r = h.response_in(id(), build_clickable);
    assert!(r.left.drag.stopped(), "the drag gets its commit edge");
    assert!(!r.left.held(), "and the press is no longer latched");
    assert_eq!(
        h.ui().input().keyboard.modifiers,
        Modifiers::default(),
        "a modifier held into another window is not held here",
    );
}
