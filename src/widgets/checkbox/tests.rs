use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn run(value: &mut bool, h: &mut UiHarness) {
    let mut v = *value;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut v)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .show(ui);
        });
    });
    *value = v;
}

#[test]
fn clicking_toggles_value() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut v = false;

    // Frame 1: lay out so the row has a rect.
    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "starts unchecked");

    // Click on the box area.
    h.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut h);
    assert!(v, "single click toggles on");

    h.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut h);
    assert!(!v, "second click toggles off");
}

#[test]
fn disabled_checkbox_does_not_toggle() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut v = false;

    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;

    h.click_at(Vec2::new(8.0, 8.0));
    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "disabled checkbox swallows click");
}
