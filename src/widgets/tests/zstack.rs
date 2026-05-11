use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::color::Color;
use crate::support::testing::{shapes_of, ui_at};
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn zstack_layers_children_without_painting_background() {
    // Like Panel but with no fill/stroke/radius — pure layered layout.
    // Wrapped in HStack so the ZStack's Hug-to-children size is honored
    // (root would otherwise expand to surface).
    let mut ui = ui_at(UVec2::new(400, 200));
    let mut zstack_node = None;
    let mut bg_node = None;
    let mut fg_node = None;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        zstack_node = Some(
            Panel::zstack()
                .id_salt("layered")
                .show(ui, |ui| {
                    bg_node = Some(
                        Frame::new()
                            .id_salt("bg")
                            .size((Sizing::Fixed(120.0), Sizing::Fixed(80.0)))
                            .background(Background {
                                fill: Color::rgb(0.1, 0.1, 0.2).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node,
                    );
                    fg_node = Some(
                        Button::new()
                            .id_salt("fg")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    ui.record_phase();
    ui.paint_phase();
    let z = zstack_node.unwrap();
    // ZStack itself paints nothing.
    assert!(shapes_of(ui.forest.tree(Layer::Main), z).next().is_none());

    // ZStack hugs to max(child sizes) = (120, 80).
    let zr = ui.layout[Layer::Main].rect[z.index()];
    assert_eq!(zr.size.w, 120.0);
    assert_eq!(zr.size.h, 80.0);

    // Both children placed at ZStack's top-left (no padding), at their own size.
    let bg = ui.layout[Layer::Main].rect[bg_node.unwrap().index()];
    let fg = ui.layout[Layer::Main].rect[fg_node.unwrap().index()];
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
        let mut ui = ui_at(UVec2::new(400, 400));
        let mut child_node = None;
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Panel::zstack()
                .id_salt("box")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
                .show(ui, |ui| {
                    child_node = Some(
                        Frame::new()
                            .id_salt("c")
                            .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                            .align(*align)
                            .background(Background {
                                fill: Color::rgb(0.5, 0.5, 0.5).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node,
                    );
                });
        });
        ui.record_phase();
        ui.paint_phase();
        let r = ui.layout[Layer::Main].rect[child_node.unwrap().index()];
        assert_eq!((r.min.x, r.min.y), *expected, "case: {label}");
        assert_eq!(
            (r.size.w, r.size.h),
            (40.0, 20.0),
            "case: {label} Fixed size honored under align"
        );
    }
}
