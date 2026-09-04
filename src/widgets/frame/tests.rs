use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn frame_paints_a_single_rounded_rect() {
    let mut h = UiHarness::new(UVec2::new(200, 100));
    let frame_node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("decoration"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .background(Background {
                        fill: RgbaF32::srgb(0.2, 0.4, 0.8).into(),
                        corners: Corners::all(6.0),
                        ..Default::default()
                    })
                    .show(ui)
                    .node()
            })
            .inner
    });
    // Chrome lives in `Tree::chrome_table`, not in the shape stream.
    assert!(
        h.ui.tree(Layer::Main)
            .shapes_of(frame_node)
            .next()
            .is_none()
    );
    assert!(
        h.ui.tree(Layer::Main).chrome(frame_node).is_some(),
        "frame chrome recorded in chrome table",
    );

    // Default sense is None — frame is not a hit-test target.
    let r = h.ui.arranged_rect(Layer::Main, frame_node);
    assert_eq!(r.size.w, 80.0);
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn frame_with_sense_click_is_clickable() {
    use glam::Vec2;

    let surface = UVec2::new(200, 100);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("hitbox"))
                .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui);
        });
    });
    h.click_at(Vec2::new(50.0, 25.0));

    let mut clicked = false;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            clicked |= Frame::new()
                .id(WidgetId::from_hash("hitbox"))
                .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                .sense(Sense::CLICK)
                .show(ui)
                .left
                .clicked();
        });
    });
    assert!(clicked);
}
