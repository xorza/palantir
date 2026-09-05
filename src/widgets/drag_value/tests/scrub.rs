//! The drag gesture: what continues it, what ends it, and what never starts
//! it.

use crate::Ui;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::drag_value::DragValue;
use crate::widgets::drag_value::tests::support::deferred_frame;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn scrub_commits_once_on_release_for_deferred_caller() {
    let id = WidgetId::from_hash("dv-scrub-commit");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;

    // Settle a layout frame so the cascade exists for pointer routing.
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Press at x=50 inside the 100×40 chip, drag 20px right:
    // draft = anchor 10 + 20px * speed 1 = 30. Live write, no commit,
    // and the deferred caller leaves canonical untouched.
    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed, "mid-drag: live write, no commit");
    assert_eq!(canonical, 10.0, "deferred caller ignores mid-drag writes");

    // 5px more: anchor math re-derives 10 + 25 = 35 even though the
    // caller re-seeded the stale 10 into the draft.
    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed);
    assert_eq!(canonical, 10.0);

    // Release: exactly one commit (in exactly one record pass), carrying
    // the final scrubbed value into the stale-seeded draft.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed, "release commits the scrub");
    assert_eq!(s.commits, 1, "one commit, one record pass");
    assert_eq!(canonical, 35.0);

    // Idle frame after: no residual signals — one commit per gesture.
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.changed && !s.committed);
    assert_eq!(canonical, 35.0);
}

#[test]
fn scrub_distance_is_scale_invariant() {
    use crate::primitives::translate_scale::TranslateScale;

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

        let response = h.ui.response_for(id);
        let layout = response.layout_rect.expect("drag value arranged");
        let press = response
            .transform
            .apply_point(layout.min + Vec2::new(50.0, 20.0));
        let drag = response
            .transform
            .apply_point(layout.min + Vec2::new(70.0, 20.0));
        h.press_at(press);
        h.move_to(drag);
        h.frame(|ui| build(ui, &mut value));

        assert_eq!(value, 30.0, "20 logical px at {scale}× must add exactly 20",);
    }
}

#[test]
fn pointer_leaving_surface_does_not_split_the_gesture() {
    // Mid-scrub window exit must not fire a premature commit, and the
    // resumed drag's remainder must still commit on the real release.
    let id = WidgetId::from_hash("dv-pointer-leave");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_at(Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    deferred_frame(&mut h, id, &mut canonical, false, false);

    // Pointer crosses the window edge: drag unobservable, but latched.
    h.pointer_left();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed, "window exit is not a release");
    assert_eq!(canonical, 10.0);

    // Re-enter with the button held and keep scrubbing: 10 + 25 = 35.
    h.move_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed && !s.committed, "resumed drag keeps writing");

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed && s.commits == 1);
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

    // One disabled frame mid-drag: no write, but the gesture survives.
    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(!s.changed && !s.committed, "disabled frame writes nothing");

    // Re-enabled with the button still held: one settle frame (the
    // cascaded disabled flag is one frame stale), then scrubbing resumes.
    deferred_frame(&mut h, id, &mut canonical, false, false);
    h.drag_to(Vec2::new(75.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.changed, "scrub resumes after the disable blip");

    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(s.committed && s.commits == 1, "release still commits");
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

    // Released on a disabled frame: a locked control emits no edit, and
    // the gesture is over — a later enabled frame must not revive it.
    h.release();
    let s = deferred_frame(&mut h, id, &mut canonical, false, true);
    assert!(!s.committed, "disabled release drops the gesture");
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed && !s.changed);
    assert_eq!(canonical, 10.0);
}

#[test]
fn non_left_drags_do_not_scrub() {
    // A right-button drag over the chip is someone else's gesture
    // (context menu, breaker) — it must neither write nor commit.
    let id = WidgetId::from_hash("dv-right-drag");
    let mut h = UiHarness::new(UVec2::new(300, 100));
    let mut canonical = 10.0_f64;
    deferred_frame(&mut h, id, &mut canonical, false, false);

    h.press_button_at(PointerButton::Right, Vec2::new(50.0, 20.0));
    h.drag_to(Vec2::new(70.0, 20.0));
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.changed && !s.committed, "right drag must not scrub");

    h.release_button(PointerButton::Right);
    let s = deferred_frame(&mut h, id, &mut canonical, false, false);
    assert!(!s.committed, "right release must not commit");
    assert_eq!(canonical, 10.0);
}

/// The sense the widget needs is folded over the caller's at `show`,
/// so no chain order can leave the chip unable to scrub.
///
/// `editable` decides only whether the click that opens the inline
/// editor joins the drag. Turning it back off drops that click again,
/// and a caller's own `Sense::SCROLL` survives either setting.
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
