use crate::layout::types::{sense::Sense, sizing::Sizing};
use crate::primitives::color::Color;
use crate::shape::Shape;
use crate::support::testing::{click_at, ui_at};
use crate::tree::element::Configure;
use crate::widgets::{frame::Frame, panel::Panel, styled::Styled};
use glam::UVec2;

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut ui = ui_at(UVec2::new(200, 100));
    let mut frame_node = None;
    Panel::hstack().show(&mut ui, |ui| {
        frame_node = Some(
            Frame::new()
                .with_id("decoration")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .radius(6.0)
                .show(ui)
                .node,
        );
    });
    ui.end_frame();

    let shapes = ui.tree.shapes.slice_of(frame_node.unwrap().index());
    assert_eq!(shapes.len(), 1);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));

    // Default sense is None — frame is not a hit-test target.
    let r = ui.pipeline.layout.result.rect[frame_node.unwrap().index()];
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn frame_with_sense_click_is_clickable() {
    use crate::layout::types::display::Display;
    use glam::Vec2;

    let mut ui = ui_at(UVec2::new(200, 100));
    Panel::hstack().show(&mut ui, |ui| {
        Frame::new()
            .with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui);
    });
    ui.end_frame();

    click_at(&mut ui, Vec2::new(50.0, 25.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        clicked = Frame::new()
            .with_id("hitbox")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .sense(Sense::CLICK)
            .show(ui)
            .clicked();
    });
    assert!(clicked);
}
