use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn zstack_hugs_to_largest_child_per_axis_independently() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(800, 600), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new().id_salt("a").size((40.0, 20.0)).show(ui);
                Frame::new().id_salt("b").size((10.0, 80.0)).show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    assert_eq!(r.size.w, 40.0);
    assert_eq!(r.size.h, 80.0);
}

#[test]
fn zstack_lays_children_at_inner_top_left_by_default() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .padding(8.0)
            .show(ui, |ui| {
                Frame::new().id_salt("a").size((20.0, 20.0)).show(ui);
                Frame::new().id_salt("b").size((30.0, 30.0)).show(ui);
            })
            .node(ui)
    });
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(panel)
        .map(|c| c.id)
        .collect();
    let panel_rect = ui.layout[Layer::Main].rect[panel.index()];
    let a = ui.layout[Layer::Main].rect[kids[0].index()];
    let b = ui.layout[Layer::Main].rect[kids[1].index()];
    assert_eq!(a.min.x, panel_rect.min.x + 8.0);
    assert_eq!(a.min.y, 8.0);
    assert_eq!(b.min.x, panel_rect.min.x + 8.0);
    assert_eq!(b.min.y, 8.0);
}

/// 100×100 ZStack under a 200×200 surface. Children's offsets relative
/// to the panel's top-left depend on `(parent_child_align, child_align)`:
/// per-axis resolution = child override else parent default else Start.
#[test]
fn zstack_per_axis_alignment() {
    type Case = (
        &'static str,
        Option<Align>,                        // parent .child_align
        Vec<((f32, f32), Align, (f32, f32))>, // (child_size, child_align, expected_offset)
    );
    let cases: Vec<Case> = vec![
        (
            "no_parent_default_two_children_full_overrides",
            None,
            vec![
                ((20.0, 20.0), Align::CENTER, (40.0, 40.0)),
                (
                    (10.0, 10.0),
                    Align::new(HAlign::Right, VAlign::Bottom),
                    (90.0, 90.0),
                ),
            ],
        ),
        (
            "parent_center_with_h_override_only",
            Some(Align::CENTER),
            // Child: 20×20, override H=Right (auto V → CENTER). Expected: x=80, y=40.
            vec![((20.0, 20.0), Align::h(HAlign::Right), (80.0, 40.0))],
        ),
    ];
    for (label, parent_align, children) in &cases {
        let mut ui = Ui::for_test();
        let panel = ui.under_outer(UVec2::new(200, 200), |ui| {
            let mut p = Panel::zstack()
                .auto_id()
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)));
            if let Some(a) = *parent_align {
                p = p.child_align(a);
            }
            p.show(ui, |ui| {
                for (i, ((w, h), align, _)) in children.iter().enumerate() {
                    Frame::new()
                        .id_salt(("c", i))
                        .size((*w, *h))
                        .align(*align)
                        .show(ui);
                }
            })
            .node(ui)
        });
        let panel_rect = ui.layout[Layer::Main].rect[panel.index()];
        let kids: Vec<_> = ui
            .forest
            .tree(Layer::Main)
            .children(panel)
            .map(|c| c.id)
            .collect();
        for (i, (_, _, expected)) in children.iter().enumerate() {
            let r = ui.layout[Layer::Main].rect[kids[i].index()];
            assert_eq!(
                (r.min.x - panel_rect.min.x, r.min.y - panel_rect.min.y),
                *expected,
                "case: {label} child[{i}]"
            );
        }
    }
}

#[test]
fn zstack_fill_child_stretches_to_inner() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("filler")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node(ui)
    });
    let panel_rect = ui.layout[Layer::Main].rect[panel.index()];
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(panel)
        .map(|c| c.id)
        .collect();
    let f = ui.layout[Layer::Main].rect[kids[0].index()];
    assert_eq!(f.min.x - panel_rect.min.x, 10.0);
    assert_eq!(f.min.y, 10.0);
    assert_eq!(f.size.w, 80.0);
    assert_eq!(f.size.h, 80.0);
}

#[test]
fn hug_zstack_with_only_fill_children_collapses_to_zero() {
    // Fill-on-both-axes children measure with INF → fall back to intrinsic;
    // a Hug ZStack therefore has no content to grow to.
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("filler")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);
}

#[test]
fn zstack_collapsed_child_does_not_grow_panel() {
    let mut ui = Ui::for_test();
    let panel = ui.under_outer(UVec2::new(400, 400), |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new().id_salt("a").size((20.0, 20.0)).show(ui);
                Frame::new()
                    .id_salt("hidden")
                    .size((100.0, 100.0))
                    .collapsed()
                    .show(ui);
            })
            .node(ui)
    });
    let r = ui.layout[Layer::Main].rect[panel.index()];
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    let kids: Vec<_> = ui
        .forest
        .tree(Layer::Main)
        .children(panel)
        .map(|c| c.id)
        .collect();
    let collapsed = ui.layout[Layer::Main].rect[kids[1].index()];
    assert_eq!(collapsed.size.w, 0.0);
    assert_eq!(collapsed.size.h, 0.0);
}
