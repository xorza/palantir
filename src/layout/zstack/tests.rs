use crate::Ui;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::test_support::under_outer;
use crate::tree::element::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn zstack_hugs_to_largest_child_per_axis_independently() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(800, 600), |ui| {
        Panel::zstack()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("a").size((40.0, 20.0)).show(ui);
                Frame::with_id("b").size((10.0, 80.0)).show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
    assert_eq!(r.size.w, 40.0);
    assert_eq!(r.size.h, 80.0);
}

#[test]
fn zstack_lays_children_at_inner_top_left_by_default() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .padding(8.0)
            .show(ui, |ui| {
                Frame::with_id("a").size((20.0, 20.0)).show(ui);
                Frame::with_id("b").size((30.0, 30.0)).show(ui);
            })
            .node
    });
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let panel_rect = ui.layout_engine.result.rect(panel);
    let a = ui.layout_engine.result.rect(kids[0]);
    let b = ui.layout_engine.result.rect(kids[1]);
    assert_eq!(a.min.x, panel_rect.min.x + 8.0);
    assert_eq!(a.min.y, 8.0);
    assert_eq!(b.min.x, panel_rect.min.x + 8.0);
    assert_eq!(b.min.y, 8.0);
}

#[test]
fn zstack_aligns_per_axis_from_child_override() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                Frame::with_id("centered")
                    .size((20.0, 20.0))
                    .align(Align::CENTER)
                    .show(ui);
                Frame::with_id("br")
                    .size((10.0, 10.0))
                    .align(Align::new(HAlign::Right, VAlign::Bottom))
                    .show(ui);
            })
            .node
    });
    let panel_rect = ui.layout_engine.result.rect(panel);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let c = ui.layout_engine.result.rect(kids[0]);
    let br = ui.layout_engine.result.rect(kids[1]);
    assert_eq!(c.min.x - panel_rect.min.x, 40.0);
    assert_eq!(c.min.y, 40.0);
    assert_eq!(br.min.x - panel_rect.min.x, 90.0);
    assert_eq!(br.min.y, 90.0);
}

#[test]
fn zstack_child_align_cascades_to_auto_axes() {
    // Parent's child_align is the default for Auto axes; child override on one
    // axis still uses the parent default on the unspecified axis.
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .child_align(Align::CENTER)
            .show(ui, |ui| {
                Frame::with_id("override-x")
                    .size((20.0, 20.0))
                    .align(Align::h(HAlign::Right))
                    .show(ui);
            })
            .node
    });
    let panel_rect = ui.layout_engine.result.rect(panel);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let r = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(r.min.x - panel_rect.min.x, 80.0);
    assert_eq!(r.min.y, 40.0);
}

#[test]
fn zstack_fill_child_stretches_to_inner() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::with_id("filler")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node
    });
    let panel_rect = ui.layout_engine.result.rect(panel);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let f = ui.layout_engine.result.rect(kids[0]);
    assert_eq!(f.min.x - panel_rect.min.x, 10.0);
    assert_eq!(f.min.y, 10.0);
    assert_eq!(f.size.w, 80.0);
    assert_eq!(f.size.h, 80.0);
}

#[test]
fn hug_zstack_with_only_fill_children_collapses_to_zero() {
    // Fill-on-both-axes children measure with INF → fall back to intrinsic;
    // a Hug ZStack therefore has no content to grow to.
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(200, 200), |ui| {
        Panel::zstack()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("filler")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);
}

#[test]
fn zstack_collapsed_child_does_not_grow_panel() {
    let mut ui = Ui::new();
    let panel = under_outer(&mut ui, UVec2::new(400, 400), |ui| {
        Panel::zstack()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("a").size((20.0, 20.0)).show(ui);
                Frame::with_id("hidden")
                    .size((100.0, 100.0))
                    .collapsed()
                    .show(ui);
            })
            .node
    });
    let r = ui.layout_engine.result.rect(panel);
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    let kids: Vec<_> = ui.tree.children(panel).collect();
    let collapsed = ui.layout_engine.result.rect(kids[1]);
    assert_eq!(collapsed.size.w, 0.0);
    assert_eq!(collapsed.size.h, 0.0);
}
