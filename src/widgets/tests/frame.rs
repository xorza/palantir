use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut ui = Ui::for_test();
    let mut frame_node = None;
    ui.run_at(UVec2::new(200, 100), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            frame_node = Some(
                Frame::new()
                    .id_salt("decoration")
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        radius: Corners::all(6.0),
                        ..Default::default()
                    })
                    .show(ui)
                    .node(ui),
            );
        });
    });
    let frame_node = frame_node.unwrap();
    // Chrome lives in `Tree::chrome_table`, not in the shape stream.
    assert!(
        ui.forest
            .tree(Layer::Main)
            .shapes_of(frame_node)
            .next()
            .is_none()
    );
    assert!(
        ui.forest.tree(Layer::Main).chrome(frame_node).is_some(),
        "frame chrome recorded in chrome table",
    );

    // Default sense is None — frame is not a hit-test target.
    let r = ui.layout[Layer::Main].rect[frame_node.index()];
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn frame_with_sense_click_is_clickable() {
    use glam::Vec2;

    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 100);
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id_salt("hitbox")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui);
        });
    });
    ui.click_at(Vec2::new(50.0, 25.0));

    let mut clicked = false;
    ui.run_at(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            clicked |= Frame::new()
                .id_salt("hitbox")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui)
                .clicked();
        });
    });
    assert!(clicked);
}
