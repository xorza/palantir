use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::types::{align::Align, align::VAlign, sizing::Sizing};
use crate::primitives::color::Color;
use crate::support::testing::{click_at, encode_cmds, run_at};
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn collapsed_child_consumes_no_space_in_hstack() {
    let mut ui = Ui::new();
    let mut root = NodeId(0);
    run_at(&mut ui, UVec2::new(400, 100), |ui| {
        root = Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Frame::new().id_salt("a").size(40.0).show(ui);
                Frame::new().id_salt("gone").size(40.0).collapsed().show(ui);
                Frame::new().id_salt("b").size(40.0).show(ui);
            })
            .node;
    });

    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let a = ui.layout[Layer::Main].rect[kids[0].index()];
    let gone = ui.layout[Layer::Main].rect[kids[1].index()];
    let b = ui.layout[Layer::Main].rect[kids[2].index()];

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
    let mut ui = Ui::new();
    let mut root = NodeId(0);
    run_at(&mut ui, UVec2::new(400, 100), |ui| {
        root = Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size((Sizing::Fill(1.0), Sizing::Hug))
                    .show(ui);
                Frame::new()
                    .id_salt("gone")
                    .size((Sizing::Fill(3.0), Sizing::Hug))
                    .collapsed()
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size((Sizing::Fill(1.0), Sizing::Hug))
                    .show(ui);
            })
            .node;
    });

    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let a = ui.layout[Layer::Main].rect[kids[0].index()];
    let b = ui.layout[Layer::Main].rect[kids[2].index()];
    // Collapsed sibling's weight (3.0) is dropped — remaining two fills split 50/50.
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hidden_keeps_slot_but_emits_no_draws() {
    use crate::renderer::frontend::cmd_buffer::CmdKind;

    let mut ui = Ui::new();
    let mut root = NodeId(0);
    run_at(&mut ui, UVec2::new(400, 100), |ui| {
        root = Panel::hstack()
            .auto_id()
            .gap(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("a")
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("hid")
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .hidden()
                    .show(ui);
                Frame::new()
                    .id_salt("b")
                    .size(40.0)
                    .background(Background {
                        fill: Color::rgb(0.0, 0.0, 1.0).into(),
                        ..Default::default()
                    })
                    .show(ui);
            })
            .node;
    });

    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(root)
        .map(|c| c.id)
        .collect();
    let hid = ui.layout[Layer::Main].rect[kids[1].index()];
    let b = ui.layout[Layer::Main].rect[kids[2].index()];
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no DrawRect.
    let cmds = encode_cmds(&ui);
    let draws = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect))
        .count();
    assert_eq!(draws, 2, "only the two Visible frames should paint");
}

#[test]
fn hidden_button_does_not_click() {
    use glam::Vec2;

    let mut ui = Ui::new();
    let surface = UVec2::new(400, 200);
    run_at(&mut ui, surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("invisible")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .hidden()
                .show(ui);
        });
    });

    click_at(&mut ui, Vec2::new(50.0, 20.0));

    let mut clicked = false;
    run_at(&mut ui, surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            clicked = Button::new()
                .id_salt("invisible")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .hidden()
                .show(ui)
                .clicked();
        });
    });
    assert!(!clicked, "hidden button should not receive clicks");
}

/// 200×100 hstack with `child_align(VAlign::Center)` and two 40×20
/// children. The first child always inherits the parent default (y=40);
/// the second child either inherits (no override → y=40) or overrides
/// (`VAlign::Bottom` → y=80). Pins both inherit-default propagation
/// and that an override on one child doesn't leak to its sibling.
#[test]
fn hstack_child_align_per_axis_with_overrides() {
    let cases: &[(&str, Option<Align>, f32)] = &[
        ("both_inherit_parent_center", None, 40.0),
        (
            "second_overrides_to_bottom",
            Some(Align::v(VAlign::Bottom)),
            80.0,
        ),
    ];
    for (label, second_override, second_y) in cases {
        let mut ui = Ui::new();
        let mut root = NodeId(0);
        run_at(&mut ui, UVec2::new(200, 100), |ui| {
            root = Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Fixed(100.0)))
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("a")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                        .show(ui);
                    let mut b = Frame::new()
                        .id_salt("b")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)));
                    if let Some(a) = *second_override {
                        b = b.align(a);
                    }
                    b.show(ui);
                })
                .node;
        });

        let kids: Vec<_> = ui
            .forest
            .tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .collect();
        let a = ui.layout[Layer::Main].rect[kids[0].index()];
        let b = ui.layout[Layer::Main].rect[kids[1].index()];
        assert_eq!(a.min.y, 40.0, "case: {label} a inherits default");
        assert_eq!(a.size.h, 20.0, "case: {label} a.size.h");
        assert_eq!(b.min.y, *second_y, "case: {label} b");
        assert_eq!(b.size.h, 20.0, "case: {label} b.size.h");
    }
}
