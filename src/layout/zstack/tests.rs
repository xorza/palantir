use crate::layout::types::{align::Align, align::HAlign, align::VAlign, sizing::Sizing};
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn zstack_hugs_to_largest_child_per_axis_independently() {
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let panel = h.under_outer(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((40.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((10.0, 80.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    assert_eq!(r.size.w, 40.0);
    assert_eq!(r.size.h, 80.0);
}

#[test]
fn zstack_lays_children_at_inner_top_left_by_default() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let panel = h.under_outer(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .padding(8.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((20.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((30.0, 30.0))
                    .show(ui);
            })
            .response
            .node()
    });
    let kids: Vec<_> = h.main_child_rects(panel);
    let panel_rect = h.ui.arranged_rect(Layer::Main, panel);
    let a = kids[0];
    let b = kids[1];
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
        let mut h = UiHarness::new(UVec2::new(200, 200));
        let panel = h.under_outer(|ui| {
            let mut p = Panel::zstack()
                .auto_id()
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)));
            if let Some(a) = *parent_align {
                p = p.child_align(a);
            }
            p.show(ui, |ui| {
                for (i, ((w, h), align, _)) in children.iter().enumerate() {
                    Frame::new()
                        .id(WidgetId::from_hash(("c", i)))
                        .size((*w, *h))
                        .align(*align)
                        .show(ui);
                }
            })
            .response
            .node()
        });
        let panel_rect = h.ui.arranged_rect(Layer::Main, panel);
        let kids: Vec<_> = h.main_child_rects(panel);
        for (i, (_, _, expected)) in children.iter().enumerate() {
            let r = kids[i];
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
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let panel = h.under_outer(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .padding(10.0)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .response
            .node()
    });
    let panel_rect = h.ui.arranged_rect(Layer::Main, panel);
    let kids: Vec<_> = h.main_child_rects(panel);
    let f = kids[0];
    assert_eq!(f.min.x - panel_rect.min.x, 10.0);
    assert_eq!(f.min.y, 10.0);
    assert_eq!(f.size.w, 80.0);
    assert_eq!(f.size.h, 80.0);
}

#[test]
fn hug_zstack_with_only_fill_children_collapses_to_zero() {
    // Fill-on-both-axes children measure with INF → fall back to intrinsic;
    // a Hug ZStack therefore has no content to grow to.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    let panel = h.under_outer(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("filler"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    assert_eq!(r.size.w, 0.0);
    assert_eq!(r.size.h, 0.0);
}

#[test]
fn zstack_collapsed_child_does_not_grow_panel() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let panel = h.under_outer(|ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((20.0, 20.0))
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("hidden"))
                    .size((100.0, 100.0))
                    .collapsed()
                    .show(ui);
            })
            .response
            .node()
    });
    let r = h.ui.arranged_rect(Layer::Main, panel);
    assert_eq!(r.size.w, 20.0);
    assert_eq!(r.size.h, 20.0);
    let kids: Vec<_> = h.main_child_rects(panel);
    let collapsed = kids[1];
    assert_eq!(collapsed.size.w, 0.0);
    assert_eq!(collapsed.size.h, 0.0);
}
