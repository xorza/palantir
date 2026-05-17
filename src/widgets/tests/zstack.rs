use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn zstack_layers_children_without_painting_background() {
    // Wrapped in HStack so the ZStack's Hug-to-children size is honored
    // (root would otherwise expand to surface).
    let mut ui = Ui::for_test();
    let mut zstack_node = None;
    let mut bg_node = None;
    let mut fg_node = None;
    ui.run_at(UVec2::new(400, 200), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            zstack_node = Some(
                Panel::zstack()
                    .id(WidgetId::from_hash("layered"))
                    .show(ui, |ui| {
                        bg_node = Some(
                            Frame::new()
                                .id(WidgetId::from_hash("bg"))
                                .size((Sizing::Fixed(120.0), Sizing::Fixed(80.0)))
                                .background(Background {
                                    fill: Color::rgb(0.1, 0.1, 0.2).into(),
                                    ..Default::default()
                                })
                                .show(ui)
                                .node(ui),
                        );
                        fg_node = Some(
                            Button::new()
                                .id(WidgetId::from_hash("fg"))
                                .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
                                .show(ui)
                                .node(ui),
                        );
                    })
                    .node(ui),
            );
        });
    });
    let z = zstack_node.unwrap();
    assert!(ui.forest.tree(Layer::Main).shapes_of(z).next().is_none());

    let zr = ui.layout[Layer::Main].rect[z.idx()];
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    let bg = ui.layout[Layer::Main].rect[bg_node.unwrap().idx()];
    let fg = ui.layout[Layer::Main].rect[fg_node.unwrap().idx()];
    assert_eq!((bg.min.x, bg.min.y), (0.0, 0.0));
    assert_eq!((fg.min.x, fg.min.y), (0.0, 0.0));
    assert_eq!((bg.size.w, bg.size.h), (120.0, 80.0));
    assert_eq!((fg.size.w, fg.size.h), (60.0, 30.0));
}

/// ZStack inner = 200×100, child = 40×20. `align(...)` resolves
/// independently per axis: Center → (100-40)/2 leading; End → inner -
/// child; Start → 0.
#[test]
fn zstack_aligns_child_per_axis() {
    let cases: &[(&str, Align, (f32, f32))] = &[
        ("center", Align::CENTER, (80.0, 40.0)),
        (
            "right_center_independent_axes",
            Align::new(HAlign::Right, VAlign::Center),
            (160.0, 40.0),
        ),
    ];
    for (label, align, expected) in cases {
        let mut ui = Ui::for_test();
        let mut child_node = None;
        ui.run_at(UVec2::new(400, 400), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("box"))
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
                    .show(ui, |ui| {
                        child_node = Some(
                            Frame::new()
                                .id(WidgetId::from_hash("c"))
                                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                                .align(*align)
                                .background(Background {
                                    fill: Color::rgb(0.5, 0.5, 0.5).into(),
                                    ..Default::default()
                                })
                                .show(ui)
                                .node(ui),
                        );
                    });
            });
        });
        let r = ui.layout[Layer::Main].rect[child_node.unwrap().idx()];
        assert_eq!((r.min.x, r.min.y), *expected, "case: {label}");
        assert_eq!(
            (r.size.w, r.size.h),
            (40.0, 20.0),
            "case: {label} Fixed size honored under align"
        );
    }
}
