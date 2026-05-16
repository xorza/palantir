use crate::UiCore;
use crate::forest::element::Configure;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn run(value: &mut bool, ui: &mut UiCore, surface: UVec2) {
    let mut v = *value;
    ui.run_at(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut v).id_salt("cb").label("label").show(ui);
        });
    });
    *value = v;
}

#[test]
fn clicking_toggles_value() {
    let mut ui = UiCore::for_test();
    let surface = UVec2::new(300, 100);
    let mut v = false;

    // Frame 1: lay out so the row has a rect.
    let mut rec = v;
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id_salt("cb")
                .label("label")
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "starts unchecked");

    // Click on the box area.
    ui.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut ui, surface);
    assert!(v, "single click toggles on");

    ui.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut ui, surface);
    assert!(!v, "second click toggles off");
}

#[test]
fn disabled_checkbox_does_not_toggle() {
    let mut ui = UiCore::for_test();
    let surface = UVec2::new(300, 100);
    let mut v = false;

    let mut rec = v;
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id_salt("cb")
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;

    ui.click_at(Vec2::new(8.0, 8.0));
    let mut rec = v;
    ui.run_at(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id_salt("cb")
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "disabled checkbox swallows click");
}
