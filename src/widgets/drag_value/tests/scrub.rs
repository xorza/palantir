//! The drag gesture: what continues it, what ends it, and what never starts it.

use crate::Ui;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::drag_value::DragValue;
use crate::widgets::drag_value::tests::support::deferred_frame;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn scrub_commits_once_on_release_for_deferred_caller() {
    let id = WidgetId::from_hash("dv-scrub-commit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;

    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Press at x=50 in the 100×40 chip, drag 20px right: draft = anchor 10 + 20 = 30; live write, no commit.
    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(
        s.a().changed && !s.a().committed,
        "mid-drag: live write, no commit"
    );
    assert_eq!(canonical, 10.0, "deferred caller ignores mid-drag writes");

    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.a().changed && !s.a().committed);
    assert_eq!(canonical, 10.0);

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.a().committed, "release commits the scrub");
    assert_eq!(
        s.count_where(|e| e.committed),
        1,
        "one commit, one record pass"
    );
    assert_eq!(canonical, 35.0);

    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.a().changed && !s.a().committed);
    assert_eq!(canonical, 35.0);
}

#[test]
fn scrub_distance_is_scale_invariant() {
    use crate::primitives::geometry::translate_scale::TranslateScale;

    let id = WidgetId::from_hash("scaled-drag-value");
    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(UVec2::new(300, 120));
        let mut value = 10.0_f64;
        let build = |ui: &mut Ui, value: &mut f64| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-drag-value-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui, |ui| {
                    DragValue::new(value)
                        .editable(false)
                        .speed(1.0)
                        .decimals(2)
                        .id(id)
                        .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                        .show(ui);
                });
        };
        h.frame(|ui| build(ui, &mut value));

        let drag = h.point_in(id, Vec2::new(70.0, 20.0));
        h.press_in(id, Vec2::new(50.0, 20.0));
        h.move_to(drag);
        h.frame(|ui| build(ui, &mut value));

        assert_eq!(value, 30.0, "20 logical px at {scale}× must add exactly 20");
    }
}

#[test]
fn pointer_leaving_surface_does_not_split_the_gesture() {
    // A mid-scrub window exit must not commit, and the resumed drag must commit on the real release.
    let id = WidgetId::from_hash("dv-pointer-leave");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.pointer_left();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.a().committed, "window exit is not a release");
    assert_eq!(canonical, 10.0);

    h.move_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(
        s.a().changed && !s.a().committed,
        "resumed drag keeps writing"
    );

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.a().committed && s.count_where(|e| e.committed) == 1);
    assert_eq!(canonical, 35.0, "one gesture, one commit, full travel");
}

#[test]
fn transient_disable_does_not_swallow_the_gesture() {
    let id = WidgetId::from_hash("dv-transient-disable");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(
        !s.a().changed && !s.a().committed,
        "disabled frame writes nothing"
    );

    // Re-enabled, button held: one settle frame (the cascaded disabled flag is stale), then scrubbing resumes.
    deferred_frame(&mut h, id, &mut canonical, false, false);
    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.a().changed, "scrub resumes after the disable blip");

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(
        s.a().committed && s.count_where(|e| e.committed) == 1,
        "release still commits"
    );
    assert_eq!(canonical, 35.0);
}

#[test]
fn release_while_disabled_drops_the_gesture() {
    let id = WidgetId::from_hash("dv-disabled-release");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Released on a disabled frame: a locked control emits no edit and the gesture is over for good.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(!s.a().committed, "disabled release drops the gesture");
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.a().committed && !s.a().changed);
    assert_eq!(canonical, 10.0);
}

/// The scrub anchors on the integer the target holds, so the whole `i64` domain survives; an `f64` anchor
/// would round values past 2^53 and a sub-step drag would store the rounded number.
#[test]
fn an_exact_integer_survives_a_scrub_that_moves_it_nowhere() {
    const EXACT: i64 = 9_007_199_254_740_993;
    const SLOW: f64 = 0.001;
    let id = WidgetId::from_hash("dv-exact-integer");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut value = EXACT;
    let frame = |h: &mut UiHarness, value: &mut i64, speed: f64| {
        h.frame(|ui| {
            DragValue::new(&mut *value)
                .speed(speed)
                .id(id)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };
    frame(&mut h, &mut value, SLOW);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    frame(&mut h, &mut value, SLOW);
    assert_eq!(value, EXACT, "a sub-step drag moves nothing");
    h.release();
    frame(&mut h, &mut value, SLOW);
    assert_eq!(value, EXACT, "and its commit stores nothing new");

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    frame(&mut h, &mut value, 1.0);
    assert_eq!(value, EXACT + 20);
}

#[test]
fn non_left_drags_do_not_scrub() {
    // A right-button drag is someone else's gesture: neither write nor commit; the left row is the control.
    let id = WidgetId::from_hash("dv-right-drag");
    for (button, scrubs) in [(PointerButton::Left, true), (PointerButton::Right, false)] {
        let mut h = UiHarness::new(UVec2::new(300, 100));
        let mut canonical = 10.0_f64;
        deferred_frame(&mut h, id, &mut canonical, false, false);

        let press = h.point_in(id, Vec2::new(50.0, 20.0));
        h.press_button_at(button, press);
        h.drag_to(press + Vec2::new(20.0, 0.0));
        let s = deferred_frame(&mut h, id, &mut canonical, false, false);
        assert_eq!(
            (s.a().changed, s.a().committed),
            (scrubs, false),
            "{button:?} drag",
        );

        h.release_button(button);
        let s = deferred_frame(&mut h, id, &mut canonical, false, false);
        assert_eq!(s.a().committed, scrubs, "{button:?} release");
        assert_eq!(canonical, if scrubs { 30.0 } else { 10.0 }, "{button:?}");
    }
}

/// The widget's sense is folded over the caller's at `show`, so no chain order leaves the chip unable to
/// scrub; `editable` only decides whether the opening click joins the drag, and `Sense::SCROLL` survives.
#[test]
fn the_widgets_own_sense_survives_every_builder_order() {
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut nodes = Vec::new();
    let mut value = 0.0_f64;
    h.frame(|ui| {
        nodes.push(
            DragValue::new(&mut value)
                .auto_id()
                .show(ui)
                .response
                .node(),
        );
        nodes.push(
            DragValue::new(&mut value)
                .editable(true)
                .auto_id()
                .show(ui)
                .response
                .node(),
        );
        nodes.push(
            DragValue::new(&mut value)
                .editable(true)
                .editable(false)
                .auto_id()
                .show(ui)
                .response
                .node(),
        );
        nodes.push(
            DragValue::new(&mut value)
                .sense(Sense::SCROLL)
                .auto_id()
                .show(ui)
                .response
                .node(),
        );
    });
    let want = [
        ("plain", Sense::DRAG),
        ("editable", Sense::CLICK | Sense::DRAG),
        ("toggled back off", Sense::DRAG),
        ("caller's own scroll", Sense::SCROLL | Sense::DRAG),
    ];
    let attrs = h.ui.tree(Layer::Main).records.attrs();
    for (node, (case, sense)) in nodes.into_iter().zip(want) {
        assert_eq!(attrs[node.idx()].sense(), sense, "case: {case}");
    }
}
