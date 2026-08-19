use crate::Ui;
use crate::display::Display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::scene::visibility::Visibility;
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, panel::Panel, spinner::Spinner};
use glam::UVec2;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum InvisibleSpinnerCase {
    Hidden,
    Collapsed,
    HiddenAncestor,
}

fn show_spinner_case(ui: &mut Ui, case: InvisibleSpinnerCase) {
    match case {
        InvisibleSpinnerCase::Hidden => {
            Spinner::new()
                .id(WidgetId::from_hash("spinner"))
                .hidden()
                .show(ui);
        }
        InvisibleSpinnerCase::Collapsed => {
            Spinner::new()
                .id(WidgetId::from_hash("spinner"))
                .collapsed()
                .show(ui);
        }
        InvisibleSpinnerCase::HiddenAncestor => {
            Panel::hstack()
                .id(WidgetId::from_hash("hidden-parent"))
                .hidden()
                .show(ui, |ui| {
                    Spinner::new().id(WidgetId::from_hash("spinner")).show(ui);
                });
        }
    }
}

#[test]
fn effectively_invisible_spinners_keep_their_shape_without_scheduling_frames() {
    let display = Display::from_physical(UVec2::new(100, 100), 1.0);

    for case in [
        InvisibleSpinnerCase::Hidden,
        InvisibleSpinnerCase::Collapsed,
        InvisibleSpinnerCase::HiddenAncestor,
    ] {
        let mut h = UiHarness::new(display.physical);
        let report = h.frame(|ui| {
            show_spinner_case(ui, case);
        });
        let tree = &h.ui.forest.trees[Layer::Main];

        assert_eq!(
            tree.shapes.records.len(),
            1,
            "{case:?}: the authored spinner shape must survive",
        );
        assert!(
            tree.paint_anims.entries.is_empty(),
            "{case:?}: an invisible spinner must have no active animation row",
        );
        assert!(
            tree.paint_anims.shape_indices.is_empty(),
            "{case:?}: an invisible spinner must have no shape animation lookup",
        );
        assert_eq!(
            report.repaint_after, None,
            "{case:?}: an invisible spinner must not schedule another frame",
        );
    }
}

#[test]
fn spinner_animation_stops_when_hidden_and_resumes_when_shown() {
    fn show_spinner(ui: &mut Ui, visibility: Visibility) {
        Spinner::new()
            .id(WidgetId::from_hash("transition-spinner"))
            .visibility(visibility)
            .show(ui);
    }

    let display = Display::from_physical(UVec2::new(100, 100), 1.0);
    let mut h = UiHarness::new(display.physical);

    let visible = h.frame(|ui| {
        show_spinner(ui, Visibility::Visible);
    });
    assert_eq!(visible.repaint_after, Some(Duration::ZERO));
    assert_eq!(h.ui.forest.trees[Layer::Main].paint_anims.entries.len(), 1,);

    h.ui.request_repaint();
    let hidden_at = Duration::from_millis(16);
    let hidden = h.at(hidden_at).frame(|ui| {
        show_spinner(ui, Visibility::Hidden);
    });
    assert_eq!(hidden.repaint_after, None);
    assert_eq!(
        h.ui.forest.trees[Layer::Main].shapes.records.len(),
        1,
        "hiding must retain the authored spinner shape",
    );
    assert!(
        h.ui.forest.trees[Layer::Main]
            .paint_anims
            .entries
            .is_empty(),
        "hiding must drop the active animation row",
    );

    h.ui.request_repaint();
    let shown_at = Duration::from_millis(32);
    let shown = h.at(shown_at).frame(|ui| {
        show_spinner(ui, Visibility::Visible);
    });
    assert_eq!(shown.repaint_after, Some(shown_at));
    assert_eq!(
        h.ui.forest.trees[Layer::Main].paint_anims.entries.len(),
        1,
        "showing must restore the active animation row",
    );
}

#[test]
fn collapsed_child_consumes_no_space_in_hstack() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let mut root = NodeId(0);
    h.frame(|ui| {
        root = Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("gone"))
                    .size(40.0)
                    .collapsed()
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size(40.0)
                    .show(ui);
            })
            .response
            .node();
    });

    let kids: Vec<_> = h.main_child_rects(root);
    let a = kids[0];
    let gone = kids[1];
    let b = kids[2];

    assert_eq!(a.min.x, 0.0);
    assert_eq!(a.size.w, 40.0);
    assert_eq!(gone.size.w, 0.0);
    assert_eq!(gone.size.h, 0.0);
    // Only one gap between the two visible siblings: 40 + 10 = 50.
    assert_eq!(b.min.x, 50.0);
    assert_eq!(b.size.w, 40.0);
}

#[test]
fn collapsed_does_not_consume_fill_weight() {
    let mut h = UiHarness::new(UVec2::new(400, 100));
    let mut root = NodeId(0);
    h.frame(|ui| {
        root = Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("gone"))
                    .size((Sizing::fill(3.0), Sizing::HUG))
                    .collapsed()
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::fill(1.0), Sizing::HUG))
                    .show(ui);
            })
            .response
            .node();
    });

    let kids: Vec<_> = h.main_child_rects(root);
    let a = kids[0];
    let b = kids[2];
    // Collapsed sibling's weight (3.0) is dropped — remaining two fills split 50/50.
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hidden_keeps_slot_but_emits_no_draws() {
    use crate::renderer::frontend::capture::PaintCall;

    let mut h = UiHarness::new(UVec2::new(400, 100));
    let mut root = NodeId(0);
    h.frame(|ui| {
        root = Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("hid"))
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .hidden()
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(0.0, 0.0, 1.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .response
            .node();
    });

    let kids: Vec<_> = h.main_child_rects(root);
    let hid = kids[1];
    let b = kids[2];
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no quad.
    let cmds = h.encode_paint();
    let draws = cmds
        .calls
        .iter()
        .filter(|command| matches!(command, PaintCall::Quad(_)))
        .count();
    assert_eq!(draws, 2, "only the two Visible frames should paint");
}

#[test]
fn hidden_button_does_not_click() {
    use glam::Vec2;

    let surface = UVec2::new(400, 200);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(WidgetId::from_hash("invisible"))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .hidden()
                .show(ui);
        });
    });

    h.click_at(Vec2::new(50.0, 20.0));

    let mut clicked = false;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            clicked = Button::new()
                .id(WidgetId::from_hash("invisible"))
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .hidden()
                .show(ui)
                .left
                .clicked();
        });
    });
    assert!(!clicked, "hidden button should not receive clicks");
}
