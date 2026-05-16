use crate::UiCore;
use crate::forest::element::Configure;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::UVec2;

fn focusable_id() -> WidgetId {
    WidgetId::from_hash("focusable")
}

fn build_focusable_leaf(ui: &mut UiCore) {
    Frame::new()
        .id_salt("focusable")
        .focusable(true)
        .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
        .show(ui);
}

#[test]
fn focused_reflects_focused_id_synchronously() {
    let mut ui = UiCore::for_test();
    ui.run_at_acked(UVec2::new(200, 200), build_focusable_leaf);
    assert!(!ui.response_for(focusable_id()).focused);

    ui.request_focus(Some(focusable_id()));
    assert!(
        ui.response_for(focusable_id()).focused,
        "focused must be true the same frame as request_focus",
    );

    ui.request_focus(None);
    assert!(!ui.response_for(focusable_id()).focused);
}

#[test]
fn disabled_reflects_cascaded_ancestor_flag() {
    let mut ui = UiCore::for_test();
    let build = |ui: &mut UiCore| {
        Panel::vstack()
            .id_salt("parent")
            .disabled(true)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("child")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
    };
    ui.run_at_acked(UVec2::new(200, 200), build);
    ui.run_at_acked(UVec2::new(200, 200), build);

    let parent_state = ui.response_for(WidgetId::from_hash("parent"));
    let child_state = ui.response_for(WidgetId::from_hash("child"));
    assert!(parent_state.disabled);
    assert!(
        child_state.disabled,
        "child inherits cascaded disabled from parent (no self flag)",
    );
}

#[test]
fn disabled_false_when_chain_clean() {
    let mut ui = UiCore::for_test();
    let build = |ui: &mut UiCore| {
        Panel::vstack().id_salt("parent").show(ui, |ui| {
            Frame::new()
                .id_salt("child")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        });
    };
    ui.run_at_acked(UVec2::new(200, 200), build);
    ui.run_at_acked(UVec2::new(200, 200), build);
    assert!(!ui.response_for(WidgetId::from_hash("child")).disabled);
}
