//! Drag-on-canvas behavior: threshold latch, position tracking,
//! click-suppression-after-drag, multi-widget isolation.
//!
//! The drag API hangs off [`crate::widgets::Response`]: callers snapshot
//! an `anchor` on `r.drag_started()` and compose `pos = anchor +
//! r.drag_delta()` each frame. `Ui::frame` re-records on action input,
//! so the dragged position lands in the same frame as the move event.

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::input::InputEvent;
use crate::input::sense::{DRAG_THRESHOLD, Sense};
use crate::input::test_support::{press_at, release_left};
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::test_support::new_ui;
use crate::ui::test_support::run_at_acked;
use crate::widgets::test_support::ResponseNodeExt;
use crate::widgets::{Response, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

const CARD_SIZE: f32 = 60.0;
const SURFACE: UVec2 = UVec2::new(400, 400);

fn card_id(label: &str) -> WidgetId {
    WidgetId::from_hash(label)
}

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
            .id_salt(self.label)
            .size((Sizing::Fixed(CARD_SIZE), Sizing::Fixed(CARD_SIZE)))
            .position(self.pos)
            .sense(Sense::DRAG)
            .show(ui);
        self.fold(&r);
    }

    // Idempotent across the multi-pass `Ui::frame` rebuild — pass 2
    // would otherwise overwrite the click with `false` and miss the
    // one-shot drag_started.
    fn fold(&mut self, r: &Response) {
        if r.drag_started() {
            self.anchor = self.pos;
        }
        if let Some(delta) = r.drag_delta() {
            self.pos = self.anchor + delta;
        }
        self.clicked |= r.clicked();
    }
}

fn frame_with(ui: &mut Ui, mut body: impl FnMut(&mut Ui)) {
    run_at_acked(ui, SURFACE, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id_salt("canvas")
                .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                .show(ui, |ui| body(ui));
        });
    });
}

#[test]
fn sub_threshold_keeps_position_and_emits_click() {
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut ui, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    press_at(&mut ui, press);
    ui.on_input(InputEvent::PointerMoved(press + Vec2::new(2.0, 2.0)));
    release_left(&mut ui);

    frame_with(&mut ui, |ui| a.record(ui));
    assert_eq!(
        a.pos,
        Vec2::new(50.0, 50.0),
        "sub-threshold leaves position"
    );
    assert!(a.clicked, "sub-threshold gesture still fires click");
}

#[test]
fn supra_threshold_moves_widget_and_suppresses_click() {
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut ui, |ui| a.record(ui));

    let press = Vec2::new(80.0, 80.0);
    let drop = press + Vec2::new(40.0, 0.0);
    press_at(&mut ui, press);
    ui.on_input(InputEvent::PointerMoved(drop));

    frame_with(&mut ui, |ui| a.record(ui));
    assert_eq!(
        a.pos,
        Vec2::new(90.0, 50.0),
        "position = anchor + delta on latch frame"
    );
    assert!(!a.clicked, "click does not fire mid-drag");

    release_left(&mut ui);
    frame_with(&mut ui, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(90.0, 50.0), "release re-grounds position");
    assert!(!a.clicked, "drag suppresses release-click");
}

#[test]
fn drag_then_release_then_drag_restarts_from_new_anchor() {
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    frame_with(&mut ui, |ui| a.record(ui));

    press_at(&mut ui, Vec2::new(80.0, 80.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(110.0, 80.0)));
    frame_with(&mut ui, |ui| a.record(ui));
    release_left(&mut ui);
    frame_with(&mut ui, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(80.0, 50.0));

    press_at(&mut ui, Vec2::new(100.0, 70.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(120.0, 80.0)));
    frame_with(&mut ui, |ui| a.record(ui));
    assert_eq!(a.pos, Vec2::new(100.0, 60.0), "second drag composes");
}

#[test]
fn only_pressed_card_moves_in_two_card_scene() {
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(20.0, 20.0));
    let mut b = Card::new("b", Vec2::new(200.0, 20.0));

    frame_with(&mut ui, |ui| {
        a.record(ui);
        b.record(ui);
    });

    press_at(&mut ui, Vec2::new(220.0, 40.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(260.0, 40.0)));

    frame_with(&mut ui, |ui| {
        a.record(ui);
        b.record(ui);
    });

    assert_eq!(a.pos, Vec2::new(20.0, 20.0), "card A undisturbed");
    assert_eq!(b.pos, Vec2::new(240.0, 20.0), "card B moves by drag delta");
}

#[test]
fn drag_started_fires_only_on_latch_frame() {
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(50.0, 50.0));
    let mut started = vec![];

    let mut step = |ui: &mut Ui, a: &mut Card| {
        let mut latched = false;
        run_at_acked(ui, SURFACE, |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::canvas()
                    .id_salt("canvas")
                    .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                    .show(ui, |ui| {
                        a.record(ui);
                        latched |= ui.response_for(card_id("a")).drag_started;
                    });
            });
        });
        started.push(latched);
    };

    step(&mut ui, &mut a);
    press_at(&mut ui, Vec2::new(80.0, 80.0));
    step(&mut ui, &mut a);
    ui.on_input(InputEvent::PointerMoved(Vec2::new(82.0, 81.0)));
    step(&mut ui, &mut a);
    let supra = Vec2::new(80.0 + DRAG_THRESHOLD + 1.0, 80.0);
    ui.on_input(InputEvent::PointerMoved(supra));
    step(&mut ui, &mut a);
    ui.on_input(InputEvent::PointerMoved(supra + Vec2::new(10.0, 0.0)));
    step(&mut ui, &mut a);

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
    let mut ui = new_ui();
    let mut a = Card::new("a", Vec2::new(40.0, 40.0));
    frame_with(&mut ui, |ui| a.record(ui));

    press_at(&mut ui, Vec2::new(60.0, 60.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(150.0, 60.0)));

    let mut card_node = None;
    run_at_acked(&mut ui, SURFACE, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::canvas()
                .id_salt("canvas")
                .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                .show(ui, |ui| {
                    let r = Frame::new()
                        .id_salt("a")
                        .size((Sizing::Fixed(CARD_SIZE), Sizing::Fixed(CARD_SIZE)))
                        .position(a.pos)
                        .sense(Sense::DRAG)
                        .show(ui);
                    card_node = Some(r.node(ui));
                    a.fold(&r);
                });
        });
    });

    let rect = ui.layout[Layer::Main].rect[card_node.unwrap().index()];
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
