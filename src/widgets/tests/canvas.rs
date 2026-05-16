use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::sizing::Sizing;
use crate::ui::test_support::new_ui;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn canvas_places_children_at_absolute_positions_and_hugs_bbox() {
    let mut ui = new_ui();
    let mut canvas_node = None;
    let mut a_node = None;
    let mut b_node = None;
    ui.run_at(UVec2::new(400, 400), |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            canvas_node = Some(
                Panel::canvas()
                    .id_salt("c")
                    .show(ui, |ui| {
                        a_node = Some(
                            Frame::new()
                                .id_salt("a")
                                .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
                                .position(Vec2::new(10.0, 5.0))
                                .show(ui)
                                .node(ui),
                        );
                        b_node = Some(
                            Frame::new()
                                .id_salt("b")
                                .size((Sizing::Fixed(30.0), Sizing::Fixed(60.0)))
                                .position(Vec2::new(80.0, 40.0))
                                .show(ui)
                                .node(ui),
                        );
                    })
                    .node(ui),
            );
        });
    });
    let c = ui.layout[Layer::Main].rect[canvas_node.unwrap().index()];
    // Hugs bbox: max(10+40, 80+30)=110, max(5+20, 40+60)=100.
    assert_eq!(c.size.w, 110.0);
    assert_eq!(c.size.h, 100.0);

    let a = ui.layout[Layer::Main].rect[a_node.unwrap().index()];
    let b = ui.layout[Layer::Main].rect[b_node.unwrap().index()];
    assert_eq!((a.min.x, a.min.y), (10.0, 5.0));
    assert_eq!((a.size.w, a.size.h), (40.0, 20.0));
    assert_eq!((b.min.x, b.min.y), (80.0, 40.0));
    assert_eq!((b.size.w, b.size.h), (30.0, 60.0));
}
