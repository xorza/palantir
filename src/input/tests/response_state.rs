use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::widget_id::WidgetId;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{begin, ui_at};
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::UVec2;

fn focusable_id() -> WidgetId {
    WidgetId::from_hash("focusable")
}

fn build_focusable_leaf(ui: &mut Ui) {
    Frame::new()
        .id_salt("focusable")
        .focusable(true)
        .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
        .show(ui);
}

/// `state.focused` reflects `Ui::input.focused == Some(id)`. No
/// one-frame lag — `request_focus` lands the same frame, unlike
/// hover/press which read the prior frame's cascade.
#[test]
fn focused_reflects_focused_id_synchronously() {
    let mut ui = ui_at(UVec2::new(200, 200));
    build_focusable_leaf(&mut ui);
    ui.post_record();
    ui.finalize_frame();
    // Initially nothing is focused.
    assert!(!ui.response_for(focusable_id()).focused);

    // request_focus updates state synchronously.
    ui.request_focus(Some(focusable_id()));
    assert!(
        ui.response_for(focusable_id()).focused,
        "focused must be true the same frame as request_focus",
    );

    // Clearing focus drops it.
    ui.request_focus(None);
    assert!(!ui.response_for(focusable_id()).focused);
}

/// `state.disabled` is the cascaded ancestor-or-self flag from
/// the *previous* frame's cascade. A child of a disabled parent
/// reads `disabled = true` even though the child's own
/// `Element::disabled` is false. Lag is acceptable (matches
/// hover/press) — widgets that need lag-free self-toggle merge
/// `state.disabled |= element.disabled` themselves.
#[test]
fn disabled_reflects_cascaded_ancestor_flag() {
    let mut ui = ui_at(UVec2::new(200, 200));
    // Frame 0: parent disabled, child not. Cascade marks both.
    Panel::vstack()
        .id_salt("parent")
        .disabled(true)
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("child")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        });
    ui.post_record();
    ui.finalize_frame();
    // Re-record so cascade is populated for response_for to read.
    begin(&mut ui, UVec2::new(200, 200));
    Panel::vstack()
        .id_salt("parent")
        .disabled(true)
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("child")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        });

    let parent_state = ui.response_for(WidgetId::from_hash("parent"));
    let child_state = ui.response_for(WidgetId::from_hash("child"));
    assert!(
        parent_state.disabled,
        "parent's own disabled flag flows into ResponseState",
    );
    assert!(
        child_state.disabled,
        "child inherits cascaded disabled from parent (no self flag)",
    );
    ui.post_record();
    ui.finalize_frame();
}

/// Conversely, with no disabled in the chain, child's
/// `state.disabled` stays false.
#[test]
fn disabled_false_when_chain_clean() {
    let mut ui = ui_at(UVec2::new(200, 200));
    Panel::vstack().id_salt("parent").show(&mut ui, |ui| {
        Frame::new()
            .id_salt("child")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .show(ui);
    });
    ui.post_record();
    ui.finalize_frame();
    begin(&mut ui, UVec2::new(200, 200));
    Panel::vstack().id_salt("parent").show(&mut ui, |ui| {
        Frame::new()
            .id_salt("child")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .show(ui);
    });
    assert!(!ui.response_for(WidgetId::from_hash("child")).disabled);
    ui.post_record();
    ui.finalize_frame();
}
