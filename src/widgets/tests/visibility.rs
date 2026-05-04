use crate::layout::types::{align::Align, align::VAlign, sizing::Sizing};
use crate::primitives::color::Color;
use crate::support::testing::{click_at, encode_cmds, ui_at};
use crate::tree::element::Configure;
use crate::widgets::{button::Button, frame::Frame, panel::Panel, styled::Styled};
use glam::UVec2;

#[test]
fn collapsed_child_consumes_no_space_in_hstack() {
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::new().with_id("a").size(40.0).show(ui);
            Frame::new().with_id("gone").size(40.0).collapsed().show(ui);
            Frame::new().with_id("b").size(40.0).show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let gone = ui.layout_engine.result.rect[kids[1].index()];
    let b = ui.layout_engine.result.rect[kids[2].index()];

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
    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("a")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
            Frame::new()
                .with_id("gone")
                .size((Sizing::Fill(3.0), Sizing::Hug))
                .collapsed()
                .show(ui);
            Frame::new()
                .with_id("b")
                .size((Sizing::Fill(1.0), Sizing::Hug))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let b = ui.layout_engine.result.rect[kids[2].index()];
    // Collapsed sibling's weight (3.0) is dropped — remaining two fills split 50/50.
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 200.0);
    assert_eq!(b.min.x, 200.0);
}

#[test]
fn hidden_keeps_slot_but_emits_no_draws() {
    use crate::renderer::frontend::cmd_buffer::CmdKind;

    let mut ui = ui_at(UVec2::new(400, 100));
    let root = Panel::hstack()
        .gap(10.0)
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("a")
                .size(40.0)
                .fill(Color::rgb(1.0, 0.0, 0.0))
                .show(ui);
            Frame::new()
                .with_id("hid")
                .size(40.0)
                .fill(Color::rgb(0.0, 1.0, 0.0))
                .hidden()
                .show(ui);
            Frame::new()
                .with_id("b")
                .size(40.0)
                .fill(Color::rgb(0.0, 0.0, 1.0))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let hid = ui.layout_engine.result.rect[kids[1].index()];
    let b = ui.layout_engine.result.rect[kids[2].index()];
    // Hidden node still occupies its slot.
    assert_eq!(hid.size.w, 40.0);
    // ...so b's offset includes hidden's width + both gaps.
    assert_eq!(b.min.x, 40.0 + 10.0 + 40.0 + 10.0);

    // ...but emits no DrawRect.
    let cmds = encode_cmds(&ui);
    let draws = cmds
        .kinds
        .iter()
        .filter(|k| matches!(k, CmdKind::DrawRect | CmdKind::DrawRectStroked))
        .count();
    assert_eq!(draws, 2, "only the two Visible frames should paint");
}

#[test]
fn hidden_button_does_not_click() {
    use crate::layout::types::display::Display;
    use glam::Vec2;

    let mut ui = ui_at(UVec2::new(400, 200));
    Panel::hstack().show(&mut ui, |ui| {
        Button::new()
            .with_id("invisible")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .hidden()
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 20.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        clicked = Button::new()
            .with_id("invisible")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .hidden()
            .show(ui)
            .clicked();
    });
    assert!(!clicked, "hidden button should not receive clicks");
}

#[test]
fn hstack_child_align_y_centers_all_children_by_default() {
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .child_align(Align::v(VAlign::Center))
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("a")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
            Frame::new()
                .with_id("b")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let a = ui.layout_engine.result.rect[kids[0].index()];
    let b = ui.layout_engine.result.rect[kids[1].index()];
    // Cross axis = 100, child = 20 tall → centered at (100-20)/2 = 40.
    assert_eq!(a.min.y, 40.0);
    assert_eq!(b.min.y, 40.0);
    assert_eq!(a.size.h, 20.0);
    assert_eq!(b.size.h, 20.0);
}

#[test]
fn child_align_self_overrides_parent_default() {
    let mut ui = ui_at(UVec2::new(200, 100));
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::Fixed(100.0)))
        .child_align(Align::v(VAlign::Center))
        .show(&mut ui, |ui| {
            Frame::new()
                .with_id("centered")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .show(ui);
            // Explicit Bottom on the child wins over the parent's default.
            Frame::new()
                .with_id("bottom")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                .align(Align::v(VAlign::Bottom))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let kids: Vec<_> = ui.tree.children(root).collect();
    let centered = ui.layout_engine.result.rect[kids[0].index()];
    let bottom = ui.layout_engine.result.rect[kids[1].index()];
    assert_eq!(centered.min.y, 40.0);
    assert_eq!(bottom.min.y, 80.0);
}
